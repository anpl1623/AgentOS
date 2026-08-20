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

use std::sync::Arc;
use std::time::Duration;

use agentos_core::agent::Agent;
use agentos_core::event::AgentEvent;
use agentos_core::ids::{TaskId, TaskRunId, TaskStepId};
use agentos_core::memory::MemoryQuery;
use agentos_core::task::{
    TaskFailure, TaskRun, TaskState, TaskStatus, TaskStep, TaskStepKind, TaskTrigger,
};
use agentos_core::trust::{Message, Role};
use agentos_persistence::{Database, ToolExecutionRecord};
use agentos_providers::{
    CompletionRequest, CompletionResponse, ProviderError, SharedProvider, message_for_tool_result,
};
use agentos_tools::{TaintTracker, ToolContext, ToolPipeline};
use tokio_util::sync::CancellationToken;

use crate::error::RuntimeError;
use crate::prompt::{PLANNING_MEMORY_KINDS, memory_message, system_prompt};
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
    cancel: CancellationToken,

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
            cancel,
            conversation: Vec::new(),
            ordinal: 0,
            last_response: None,
            pending_failure: None,
            recovery_attempts: 0,
        }
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
        self.seed_conversation(objective).await?;

        self.machine
            .emit(AgentEvent::TaskStarted {
                objective: objective.to_owned(),
                attempt: self.run.attempt,
            })
            .await;
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

            self.machine.apply(trigger).await?;
        }

        self.finish(started).await
    }

    async fn seed_conversation(&mut self, objective: &str) -> Result<(), RuntimeError> {
        let memories = self
            .database
            .memories()
            .query(&MemoryQuery::for_agent(self.agent.id).of_kinds(PLANNING_MEMORY_KINDS))
            .await?;

        if let Some(message) = memory_message(&memories) {
            self.conversation.push(message);
        }
        self.conversation.push(Message::objective(objective));
        Ok(())
    }

    /// Ask the model what to do next.
    async fn plan(&mut self) -> Result<TaskTrigger, RuntimeError> {
        if self.run.steps_taken >= self.agent.max_steps {
            self.pending_failure = Some(TaskFailure::StepBudgetExhausted {
                limit: self.agent.max_steps,
            });
            return Ok(TaskTrigger::UnrecoverableError);
        }

        let tools = self
            .pipeline
            .registry()
            .metadata_for(&self.agent.enabled_tools);

        let request = CompletionRequest::new(
            &self.agent.model.model,
            system_prompt(&self.agent.instructions),
        )
        .with_messages(self.conversation.clone())
        .with_tools(tools.clone())
        .with_max_output_tokens(self.agent.model.max_output_tokens)
        .with_temperature(self.agent.model.temperature);

        self.machine
            .emit(AgentEvent::ModelRequestStarted {
                provider: self.provider.id().to_owned(),
                model: self.agent.model.model.clone(),
                message_count: self.conversation.len(),
                tool_count: tools.len(),
            })
            .await;

        let started = std::time::Instant::now();
        let response = match self.call_provider(request).await {
            Ok(response) => response,
            Err(error) => {
                self.machine
                    .emit(AgentEvent::ModelRequestFailed {
                        provider: self.provider.id().to_owned(),
                        error: error.to_string(),
                    })
                    .await;

                if matches!(error, ProviderError::Cancelled) {
                    return Ok(TaskTrigger::Cancel);
                }

                self.pending_failure = Some(TaskFailure::Provider {
                    message: error.to_string(),
                });
                return Ok(
                    if error.is_retryable() && self.recovery_attempts < PROVIDER_RETRIES {
                        TaskTrigger::RecoverableError
                    } else {
                        TaskTrigger::UnrecoverableError
                    },
                );
            }
        };

        self.run.steps_taken += 1;
        self.run.input_tokens += response.usage.input_tokens.unwrap_or(0);
        self.run.output_tokens += response.usage.output_tokens.unwrap_or(0);
        self.database.runs().update(&self.run).await?;

        self.machine
            .emit(AgentEvent::ModelRequestCompleted {
                provider: self.provider.id().to_owned(),
                model: self.agent.model.model.clone(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                tool_calls: response.tool_calls().len(),
            })
            .await;

        let text = response.text();
        self.machine
            .emit(AgentEvent::ReasoningCompleted {
                summary: truncate(&text, 400),
                tool_calls: response.tool_calls().len(),
            })
            .await;

        self.record_step(
            TaskStepKind::Planning,
            if text.is_empty() {
                format!("Requested {} tool call(s)", response.tool_calls().len())
            } else {
                truncate(&text, 200)
            },
            None,
            Some(serde_json::json!({
                "text": text,
                "tool_calls": response.tool_calls().len(),
            })),
        )
        .await?;

        // The model's own turn goes back into the conversation as model output,
        // never as control-plane content.
        self.conversation
            .push(Message::new(Role::Assistant, response.content.clone()));

        let wants_tools = response.wants_tools();
        self.last_response = Some(response);
        self.recovery_attempts = 0;

        Ok(if wants_tools {
            TaskTrigger::PlanProducedToolCalls
        } else {
            TaskTrigger::PlanProducedNoToolCalls
        })
    }

    async fn call_provider(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let mut attempt = 0;
        loop {
            match self
                .provider
                .complete(request.clone(), self.cancel.clone())
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if error.is_retryable() && attempt < PROVIDER_RETRIES => {
                    attempt += 1;
                    tracing::warn!(%error, attempt, "retrying provider request");
                    tokio::select! {
                        () = self.cancel.cancelled() => return Err(ProviderError::Cancelled),
                        () = tokio::time::sleep(PROVIDER_RETRY_DELAY * attempt) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Run the tools the model asked for.
    async fn execute(&mut self) -> Result<TaskTrigger, RuntimeError> {
        let calls = self
            .last_response
            .as_ref()
            .map(CompletionResponse::tool_calls)
            .unwrap_or_default();

        for call in calls {
            if self.cancel.is_cancelled() {
                return Ok(TaskTrigger::Cancel);
            }

            let report = self
                .pipeline
                .execute(
                    &call,
                    &self.context,
                    &self.taint,
                    &self.agent.name,
                    &self.agent.enabled_tools,
                    &self.cancel,
                )
                .await;

            self.database
                .executions()
                .insert(&ToolExecutionRecord {
                    id: report.execution_id,
                    run_id: self.run.id,
                    tool: report.tool.clone(),
                    call_id: report.call_id.clone(),
                    arguments: report.arguments.clone(),
                    outcome: report.outcome,
                    effect: report.effect,
                    risk: report.risk,
                    tainted: report.tainted,
                    approval_id: report.approval_id,
                    output_bytes: report.output_bytes,
                    error: report.error.clone(),
                    duration_ms: report.duration_ms,
                    started_at: report.started_at,
                    completed_at: Some(report.completed_at),
                })
                .await?;

            let summary = report.plan.as_ref().map_or_else(
                || format!("{} → {}", report.tool, report.outcome.as_str()),
                |plan| format!("{} → {}", plan.summary, report.outcome.as_str()),
            );
            self.record_step(
                TaskStepKind::ToolCall,
                summary,
                Some(report.execution_id),
                Some(serde_json::json!({
                    "tool": report.tool,
                    "outcome": report.outcome.as_str(),
                    "effect": report.effect.as_str(),
                    "risk": report.risk.as_str(),
                    "duration_ms": report.duration_ms,
                })),
            )
            .await?;

            // The result goes back as untrusted data, whether it succeeded or
            // was refused. A refusal the model can read is what lets it re-plan.
            self.conversation
                .push(message_for_tool_result(&report.result));

            if self.taint.is_tainted() && !self.run.tainted {
                self.run.tainted = true;
                self.database.runs().update(&self.run).await?;
            }

            if matches!(report.outcome, agentos_core::tool::ToolOutcome::Cancelled)
                && self.cancel.is_cancelled()
            {
                return Ok(TaskTrigger::Cancel);
            }
        }

        Ok(TaskTrigger::ToolsCompleted)
    }

    /// Decide whether the objective has been met.
    ///
    /// The signal is the model declining to call further tools: it has said it
    /// is finished. A stronger check — a second model pass judging the result —
    /// belongs here later, and this is the seam for it.
    fn verify(&self) -> TaskTrigger {
        match self.last_response.as_ref() {
            Some(response) if !response.wants_tools() => TaskTrigger::VerificationPassed,
            Some(_) => TaskTrigger::VerificationNeedsMoreWork,
            None => TaskTrigger::VerificationNeedsMoreWork,
        }
    }

    async fn recover(&mut self) -> TaskTrigger {
        self.recovery_attempts += 1;
        let _ = self
            .record_step(
                TaskStepKind::Recovery,
                format!(
                    "Recovering from: {}",
                    self.pending_failure
                        .as_ref()
                        .map_or_else(|| "unknown error".to_owned(), ToString::to_string)
                ),
                None,
                None,
            )
            .await;

        if self.recovery_attempts > PROVIDER_RETRIES {
            TaskTrigger::UnrecoverableError
        } else {
            self.pending_failure = None;
            TaskTrigger::RecoverySucceeded
        }
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

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut cut = max;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}
