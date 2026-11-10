//! The agent loop.
//!
//! Explicitly a state machine, not one large function. Each state is a small
//! method that does one thing and returns the trigger for the next transition;
//! [`RunStateMachine`] applies it, persists it and emits an event. The loop
//! below is therefore mostly a `match` — all the interesting behaviour is in
//! the individual state handlers, and every move between them is observable.
//!
//! ```text
//! Idle → Planning ⇄ Executing → Observing → Verifying → Completed
//!            ↑           ↓                      │
//!            │   WaitingForApproval             │
//!            └───────────┴──── more work ───────┘
//! ```
//!
//! The loop, and what a run starts and ends with, are here. Each state's
//! handler is beside the others like it: what the model is shown
//! (`conversation`), asking it what to do (`planning`), and acting on the
//! answer, judging it and recovering from a failed turn (`acting`).

mod acting;
mod conversation;
mod planning;

use std::sync::Arc;
use std::time::Duration;

use agentos_core::agent::Agent;
use agentos_core::event::AgentEvent;
use agentos_core::ids::{TaskId, TaskRunId, TaskStepId};
use agentos_core::task::{
    TaskFailure, TaskRun, TaskState, TaskStatus, TaskStep, TaskStepKind, TaskTrigger,
};
use agentos_core::trust::Message;
use agentos_persistence::Database;
use agentos_providers::{CompletionResponse, SharedProvider};
use agentos_tools::{TaintTracker, ToolContext, ToolPipeline};
use tokio_util::sync::CancellationToken;

use crate::budget::RunBudget;
use crate::error::RuntimeError;
use crate::state::RunStateMachine;

/// How many times a retryable provider error is retried before giving up.
pub const PROVIDER_RETRIES: u32 = 2;

/// Base delay between provider retries; multiplied by the attempt number.
pub const PROVIDER_RETRY_DELAY: Duration = Duration::from_millis(400);

/// What a completed run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// The run.
    pub run_id: TaskRunId,
    /// The task.
    pub task_id: TaskId,
    /// Where it ended up.
    pub state: TaskState,
    /// The agent's final report, when it finished successfully.
    pub result: Option<String>,
    /// Why it failed, when it did.
    pub failure: Option<TaskFailure>,
    /// Model turns consumed.
    pub steps: u32,
    /// Whether the run ingested untrusted data.
    pub tainted: bool,
    /// Input tokens, when reported.
    pub input_tokens: u64,
    /// Output tokens, when reported.
    pub output_tokens: u64,
}

impl RunOutcome {
    /// Whether the run succeeded.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self.state, TaskState::Completed)
    }
}

/// What [`AgentEvent::TaintRaised`] names as the way in when the untrusted data
/// arrived through recalled memory rather than through a tool call.
const MEMORY_CHANNEL: &str = "memory";

/// What [`AgentEvent::TaintRaised`] names as the way in when the run inherited
/// its taint from an earlier attempt at the same task.
const RETRY_CHANNEL: &str = "retry";

/// What [`AgentEvent::TaintRaised`] names as the way in when the run inherited
/// its taint from the run that delegated to it.
const DELEGATION_CHANNEL: &str = "agent.delegate";

/// Drives one run from start to finish.
pub struct AgentLoop {
    agent: Agent,
    run: TaskRun,
    database: Database,
    provider: SharedProvider,
    pipeline: ToolPipeline,
    machine: Arc<RunStateMachine>,
    context: ToolContext,
    taint: Arc<TaintTracker>,
    budget: Arc<RunBudget>,
    cancel: CancellationToken,
    /// How the taint this run starts with reached it, for its own trail.
    inherited_through: &'static str,

    conversation: Vec<Message>,
    ordinal: u32,
    last_response: Option<CompletionResponse>,
    pending_failure: Option<TaskFailure>,
    recovery_attempts: u32,
}

impl std::fmt::Debug for AgentLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoop")
            .field("agent", &self.agent.name)
            .field("run", &self.run.id)
            .field("steps", &self.run.steps_taken)
            .finish_non_exhaustive()
    }
}

impl AgentLoop {
    /// Assemble a loop for one run.
    #[allow(
        clippy::too_many_arguments,
        reason = "composition root; every field is required"
    )]
    #[must_use]
    pub fn new(
        agent: Agent,
        run: TaskRun,
        database: Database,
        provider: SharedProvider,
        pipeline: ToolPipeline,
        machine: Arc<RunStateMachine>,
        context: ToolContext,
        taint: Arc<TaintTracker>,
        budget: Arc<RunBudget>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            agent,
            run,
            database,
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            cancel,
            inherited_through: RETRY_CHANNEL,
            conversation: Vec::new(),
            ordinal: 0,
            last_response: None,
            pending_failure: None,
            recovery_attempts: 0,
        }
    }

    /// Mark this run as one another run delegated to.
    ///
    /// Its record starts with whatever its delegator had read, and the trail
    /// says so under that name rather than as a retry it never was.
    #[must_use]
    pub(crate) const fn delegated(mut self) -> Self {
        self.inherited_through = DELEGATION_CHANNEL;
        self
    }

    /// Run to a terminal state.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] only for failures of the runtime itself — a database
    /// write, an illegal transition. A task that simply fails returns `Ok` with
    /// a failed [`RunOutcome`]: an agent being unable to do something is a
    /// result, not an exception.
    pub async fn run(mut self, objective: &str) -> Result<RunOutcome, RuntimeError> {
        // A retry starts with what earlier attempts read, and a delegated run
        // with what its delegator read. Its tracker is already tainted, so
        // nothing in this run will flip it; without a record here its own
        // trail would never say why it began that way.
        let inherited = self.run.taint_sources.clone();
        let raised_by = self.seed_conversation(objective).await?;

        self.machine
            .emit(AgentEvent::TaskStarted {
                objective: objective.to_owned(),
                attempt: self.run.attempt,
            })
            .await;
        for source in inherited {
            self.machine
                .emit(AgentEvent::TaintRaised {
                    source,
                    tool: self.inherited_through.to_owned(),
                })
                .await;
        }
        if let Some(source) = raised_by {
            self.machine
                .emit(AgentEvent::TaintRaised {
                    source,
                    tool: MEMORY_CHANNEL.to_owned(),
                })
                .await;
        }
        self.sync_taint().await?;
        self.database
            .tasks()
            .set_status(self.run.task_id, TaskStatus::Running)
            .await?;

        let started = std::time::Instant::now();
        self.machine.apply(TaskTrigger::Start).await?;

        loop {
            if self.cancel.is_cancelled() && !self.machine.current().await.is_terminal() {
                self.machine.try_apply(TaskTrigger::Cancel).await;
            }

            let state = self.machine.current().await;
            if state.is_terminal() {
                break;
            }

            let trigger = match state {
                TaskState::Planning => self.plan().await?,
                TaskState::Executing => self.execute().await?,
                TaskState::Observing => TaskTrigger::ObservationRecorded,
                TaskState::Verifying => self.verify(),
                TaskState::Recovering => self.recover().await,
                // Approvals move the machine from inside the gate; reaching
                // here means the gate returned without resolving, which is a
                // bug rather than a state to sit in.
                TaskState::WaitingForApproval => TaskTrigger::UnrecoverableError,
                TaskState::Idle
                | TaskState::Completed
                | TaskState::Failed
                | TaskState::Cancelled => {
                    break;
                }
            };

            // The gate moves the machine itself, and a run cancelled while a
            // person was deciding is already `Cancelled` by the time the
            // handler returns. Applying the handler's trigger on top would be
            // an illegal transition out of a terminal state, and would skip
            // the clean-up below that a cancelled run needs most.
            if self.machine.current().await.is_terminal() {
                break;
            }
            self.machine.apply(trigger).await?;
        }

        self.finish(started).await
    }

    async fn finish(mut self, started: std::time::Instant) -> Result<RunOutcome, RuntimeError> {
        // Release anything the tools were holding for this run — a browser
        // process, most notably. Done first so that a later failure writing the
        // outcome cannot leak a subprocess.
        self.pipeline.registry().end_run(self.run.id).await;

        let state = self.machine.current().await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let result = matches!(state, TaskState::Completed)
            .then(|| {
                self.last_response
                    .as_ref()
                    .map(CompletionResponse::text)
                    .filter(|text| !text.is_empty())
            })
            .flatten();

        self.run.state = state;
        self.run.result.clone_from(&result);
        self.run.failure.clone_from(&self.pending_failure);
        self.run.tainted = self.taint.is_tainted();
        self.run.taint_sources = self.taint.sources();
        if self.run.completed_at.is_none() {
            self.run.completed_at = Some(agentos_core::now());
        }
        self.database.runs().update(&self.run).await?;
        self.database
            .tasks()
            .set_status(self.run.task_id, TaskStatus::from_run_state(state))
            .await?;

        match state {
            TaskState::Completed => {
                self.machine
                    .emit(AgentEvent::TaskCompleted {
                        steps: self.run.steps_taken,
                        duration_ms,
                    })
                    .await;
            }
            TaskState::Cancelled => {
                // Nobody can usefully answer an approval for a run that has
                // stopped, so they are cleared rather than left in the queue.
                let _ = self
                    .database
                    .approvals()
                    .cancel_pending_for_run(self.run.id)
                    .await;
                self.machine
                    .emit(AgentEvent::TaskCancelled {
                        steps: self.run.steps_taken,
                    })
                    .await;
            }
            _ => {
                self.machine
                    .emit(AgentEvent::TaskFailed {
                        reason: self
                            .pending_failure
                            .as_ref()
                            .map_or_else(|| "unknown".to_owned(), ToString::to_string),
                        steps: self.run.steps_taken,
                    })
                    .await;
            }
        }

        Ok(RunOutcome {
            run_id: self.run.id,
            task_id: self.run.task_id,
            state,
            result,
            failure: self.pending_failure,
            steps: self.run.steps_taken,
            tainted: self.run.tainted,
            input_tokens: self.run.input_tokens,
            output_tokens: self.run.output_tokens,
        })
    }

    async fn record_step(
        &mut self,
        kind: TaskStepKind,
        summary: String,
        tool_execution_id: Option<agentos_core::ids::ToolExecutionId>,
        detail: Option<serde_json::Value>,
    ) -> Result<(), RuntimeError> {
        self.ordinal += 1;
        let step = TaskStep {
            id: TaskStepId::new(),
            run_id: self.run.id,
            ordinal: self.ordinal,
            kind,
            state: self.machine.current().await,
            summary,
            tool_execution_id,
            detail,
            at: agentos_core::now(),
        };
        self.database.steps().insert(&step).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::agent::ModelConfig;
    use agentos_providers::{MockProvider, ScriptedTurn};
    use agentos_secrets::InMemorySecretStore;
    use agentos_tools::RecordingGate;

    use crate::{FixedProviderFactory, Runtime};

    use super::*;

    /// An agent named `solo`, offered `filesystem.list`, under `policy` when
    /// one is given, answered by `script`.
    async fn solo(
        dir: &tempfile::TempDir,
        script: Vec<ScriptedTurn>,
        policy: Option<&str>,
    ) -> (Runtime, agentos_core::agent::Agent) {
        let mut runtime = Runtime::in_memory(
            std::fs::canonicalize(dir.path()).unwrap(),
            Arc::new(InMemorySecretStore::new()),
        )
        .await
        .unwrap();
        runtime.set_provider_factory(Arc::new(FixedProviderFactory::new(Arc::new(
            MockProvider::new(script),
        ))));
        let agent = runtime
            .create_agent(
                "solo",
                "i",
                ModelConfig::new("mock", "m"),
                vec!["filesystem.list".to_owned()],
            )
            .await
            .unwrap();
        if let Some(policy) = policy {
            runtime
                .database()
                .agents()
                .set_policy(agent.id, policy)
                .await
                .unwrap();
        }
        (runtime, agent)
    }

    #[tokio::test]
    async fn a_run_stopped_while_a_person_decides_ends_cancelled_rather_than_in_error() {
        #[derive(Debug)]
        struct StoppedWhileDeciding;

        #[async_trait::async_trait]
        impl agentos_tools::ApprovalGate for StoppedWhileDeciding {
            async fn request(
                &self,
                _request: &agentos_core::approval::ApprovalRequest,
                _cancel: CancellationToken,
            ) -> agentos_tools::ApprovalOutcome {
                agentos_tools::ApprovalOutcome::Cancelled
            }
        }

        let dir = tempfile::TempDir::new().unwrap();
        let (runtime, agent) = solo(
            &dir,
            vec![ScriptedTurn::call(
                "c1",
                "filesystem.list",
                serde_json::json!({"path": "."}),
            )],
            Some("default: ask\npermissions: {}\n"),
        )
        .await;

        let outcome = runtime
            .run_objective(
                agent.id,
                "List.",
                Arc::new(StoppedWhileDeciding),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.state, TaskState::Cancelled, "{outcome:?}");
        assert!(runtime.running_runs().await.is_empty());
    }

    /// A run of an agent allowed one turn, whose provider fails until the
    /// loop has given up on it once and then answers.
    async fn run_with_one_step(script: Vec<ScriptedTurn>) -> RunOutcome {
        let dir = tempfile::TempDir::new().unwrap();
        let (runtime, mut agent) = solo(&dir, script, None).await;
        agent.max_steps = 1;
        runtime.database().agents().update(&agent).await.unwrap();

        runtime
            .run_objective(
                agent.id,
                "Answer.",
                Arc::new(RecordingGate::approving()),
                CancellationToken::new(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_turn_the_provider_never_produced_is_not_spent() {
        // Every attempt the loop makes at its first turn fails, so it recovers
        // and plans again. The failed turn was never taken: before the budget
        // was shared it cost nothing, and a run allowed one turn still gets it.
        let failing = (0..=PROVIDER_RETRIES).map(|_| ScriptedTurn::Error("overloaded".into()));
        let outcome = run_with_one_step(
            failing
                .chain(std::iter::once(ScriptedTurn::text("Answered.")))
                .collect(),
        )
        .await;

        assert_eq!(outcome.state, TaskState::Completed, "{outcome:?}");
        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.result.as_deref(), Some("Answered."));
    }
}
