//! One child of a call: recorded, started, driven to its end and reported.
//!
//! The order of events the module documentation describes is kept here: a
//! child's task, its `delegations` row and its run exist, and the record
//! sealing the edge between the two runs is written, before it does anything.

use std::sync::Arc;

use agentos_core::delegation::{Delegation, DelegationState};
use agentos_core::event::AgentEvent;
use agentos_core::ids::{DelegationId, TaskId, TaskRunId};
use agentos_core::task::{Task, TaskState, TaskStatus};
use agentos_core::trust::{DataSource, UntrustedContent};
use agentos_tools::ToolOutput;
use tokio_util::sync::CancellationToken;

use super::DelegateRequest;
use super::broker::ParentRunHandle;
use super::tool::{Vetted, vet};
use crate::{PreparedRun, RunOutcome, RuntimeError};

/// One child of a call: started, or not.
pub(super) enum Child {
    /// Running on a task of its own, which records how it ends.
    Started {
        handle: tokio::task::JoinHandle<Report>,
        cancel: CancellationToken,
        /// What to report if the task itself is lost.
        fallback: Report,
    },
    /// Never started; already recorded as far as it got.
    NotStarted(Report),
}

impl Child {
    pub(super) async fn finish(self) -> Report {
        match self {
            Self::NotStarted(report) => report,
            Self::Started {
                handle, fallback, ..
            } => match handle.await {
                Ok(report) => report,
                Err(error) => Report {
                    state: DelegationState::Failed,
                    failure: Some(format!("the run stopped without finishing: {error}")),
                    ..fallback
                },
            },
        }
    }
}

/// What a supervisor reads about one child.
#[derive(Debug, Clone)]
pub(super) struct Report {
    delegation_id: Option<DelegationId>,
    agent: String,
    child_task_id: Option<TaskId>,
    child_run_id: Option<TaskRunId>,
    state: DelegationState,
    steps: u32,
    tainted: bool,
    result: Option<String>,
    failure: Option<String>,
}

impl Report {
    fn not_started(agent: &str, failure: impl Into<String>) -> Self {
        Self {
            delegation_id: None,
            agent: agent.to_owned(),
            child_task_id: None,
            child_run_id: None,
            state: DelegationState::Refused,
            steps: 0,
            tainted: false,
            result: None,
            failure: Some(failure.into()),
        }
    }

    /// The text the supervising model reads.
    fn text(&self) -> String {
        let why = self.failure.as_deref().unwrap_or("no reason was recorded");
        match self.state {
            DelegationState::Completed => self
                .result
                .clone()
                .unwrap_or_else(|| format!("{} finished without a report.", self.agent)),
            DelegationState::Cancelled => {
                format!("{} was cancelled before it finished.", self.agent)
            }
            DelegationState::Refused | DelegationState::Requested => {
                format!("{} was not started: {why}", self.agent)
            }
            DelegationState::Failed | DelegationState::Running => {
                format!("{} failed: {why}", self.agent)
            }
        }
    }

    fn structured(&self) -> serde_json::Value {
        serde_json::json!({
            "delegation_id": self.delegation_id.map(|id| id.to_string()),
            "agent": self.agent,
            "child_task_id": self.child_task_id.map(|id| id.to_string()),
            "child_run_id": self.child_run_id.map(|id| id.to_string()),
            "state": self.state.as_str(),
            "steps": self.steps,
            "tainted": self.tainted,
            "result": self.result,
            "failure": self.failure,
        })
    }
}

/// Record and start one child, or record why it did not start.
///
/// Nothing a child does happens before its task, its `delegations` row and
/// its run exist, and the audit record naming both runs has been written.
pub(super) async fn start(handle: &Arc<ParentRunHandle>, request: DelegateRequest) -> Child {
    let runtime = &handle.runtime;
    let database = runtime.database();

    // Again, because the agents table may have changed since the plan.
    let child_agent = match vet(handle, &request.agent).await {
        Ok(agent) => agent,
        Err(Vetted::Refused(refusal)) => {
            handle.record_refusal(&request.agent, &refusal).await;
            return Child::NotStarted(Report::not_started(&request.agent, refusal.to_string()));
        }
        Err(Vetted::Unreadable(error)) => {
            return Child::NotStarted(Report::not_started(
                &request.agent,
                format!("the agents could not be read: {error}"),
            ));
        }
    };

    // Born cancelled, and claimed from there with its run. A task nobody has
    // run is `pending`, and a scheduler starts pending tasks: one that read
    // this task in the moment before its run was written would carry it out
    // unattended, under the child's own policy alone, outside the
    // intersection. No scheduler starts a cancelled task, and a run may still
    // be claimed from one.
    let mut task = Task::new(child_agent.id, &request.objective).with_parent(handle.task_id);
    task.status = TaskStatus::Cancelled;
    if let Err(error) = database.tasks().insert(&task).await {
        tracing::error!(%error, "failed to record a delegated task");
        return Child::NotStarted(Report::not_started(
            &child_agent.name,
            "the delegation could not be recorded, so it was not performed",
        ));
    }

    let delegation = Delegation::requested(
        handle.run_id,
        handle.agent.id,
        child_agent.id,
        task.id,
        request.objective,
        handle.depth.saturating_add(1),
    );
    if let Err(error) = database.delegations().insert(&delegation).await {
        tracing::error!(%error, "failed to record a delegation");
        return Child::NotStarted(Report {
            child_task_id: Some(task.id),
            ..Report::not_started(
                &child_agent.name,
                "the delegation could not be recorded, so it was not performed",
            )
        });
    }

    let mut report = Report {
        delegation_id: Some(delegation.id),
        agent: child_agent.name.clone(),
        child_task_id: Some(task.id),
        child_run_id: None,
        state: DelegationState::Requested,
        steps: 0,
        tainted: false,
        result: None,
        failure: None,
    };

    let prepared = match runtime
        .prepare_delegated_run(handle, &delegation, child_agent)
        .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            report.state = match error {
                RuntimeError::Delegation(_)
                | RuntimeError::DisabledAgent(_)
                | RuntimeError::UnknownAgent(_) => DelegationState::Refused,
                _ => DelegationState::Failed,
            };
            report.failure = Some(format!("its run could not be started: {error}"));
            close(handle, delegation.id, report.state, 0).await;
            return Child::NotStarted(report);
        }
    };
    let child_run_id = prepared.run.id;
    report.child_run_id = Some(child_run_id);

    let sealed = match database
        .delegations()
        .attach_run(delegation.id, child_run_id)
        .await
    {
        Ok(()) => handle
            .record(AgentEvent::AgentDelegated {
                delegation_id: delegation.id,
                parent_run_id: handle.run_id,
                child_agent: report.agent.clone(),
                child_task_id: task.id,
                child_run_id,
                objective: delegation.objective.clone(),
                depth: delegation.depth,
            })
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(error) = sealed {
        // The run exists and is claimed, so it is ended properly rather than
        // abandoned: driven under a cancelled token, which stops it before it
        // asks a model anything.
        tracing::error!(%error, "a delegation could not be sealed; stopping its run unstarted");
        prepared.cancel.cancel();
        let _ = runtime.drive(prepared).await;
        report.state = DelegationState::Failed;
        report.failure = Some(
            "the delegation could not be recorded, so its run was stopped before it began"
                .to_owned(),
        );
        close(handle, delegation.id, report.state, 0).await;
        return Child::NotStarted(report);
    }

    let cancel = prepared.cancel.clone();
    let fallback = report.clone();
    let parent = Arc::clone(handle);
    let delegation_id = delegation.id;
    let task =
        tokio::spawn(async move { conclude(&parent, delegation_id, prepared, report).await });
    Child::Started {
        handle: task,
        cancel,
        fallback,
    }
}

/// Drive a child to its end and record how it ended.
async fn conclude(
    parent: &ParentRunHandle,
    delegation_id: DelegationId,
    prepared: PreparedRun,
    mut report: Report,
) -> Report {
    let run_id = prepared.run.id;
    let (state, run_state) = match parent.runtime.drive(prepared).await {
        Ok(outcome) => {
            let state = settled(&outcome);
            report.steps = outcome.steps;
            report.tainted = outcome.tainted;
            report.result = outcome.result;
            report.failure = outcome.failure.map(|failure| failure.to_string());
            (state, outcome.state)
        }
        Err(error) => {
            report.failure = Some(format!("the runtime failed while it worked: {error}"));
            (DelegationState::Failed, TaskState::Failed)
        }
    };
    report.state = state;
    if !report.tainted {
        // The tracker is shared: whatever the child read, or the parent had
        // read before it, is on it.
        report.tainted = parent.taint.is_tainted();
    }

    close(parent, delegation_id, state, report.steps).await;
    if let Err(error) = parent
        .record(AgentEvent::DelegationCompleted {
            delegation_id,
            child_run_id: run_id,
            state: run_state,
            steps: report.steps,
            tainted: report.tainted,
        })
        .await
    {
        tracing::error!(%error, "failed to record a delegation ending");
    }
    report
}

/// The delegation state a child's outcome amounts to.
const fn settled(outcome: &RunOutcome) -> DelegationState {
    match outcome.state {
        TaskState::Completed => DelegationState::Completed,
        TaskState::Cancelled => DelegationState::Cancelled,
        _ => DelegationState::Failed,
    }
}

/// Mark a delegation finished, saying so in the log if the row will not take it.
async fn close(handle: &ParentRunHandle, id: DelegationId, state: DelegationState, steps: u32) {
    if let Err(error) = handle
        .runtime
        .database()
        .delegations()
        .complete(id, state, steps)
        .await
    {
        tracing::error!(%error, delegation = %id, "failed to record a delegation's end");
    }
}

/// The call's result: each child's report, untrusted and labelled with the
/// agent that wrote it.
///
/// With several children each report is sealed in its own envelope, so that
/// one child's prose cannot pass itself off as the end of another's.
pub(super) fn output(reports: &[Report]) -> ToolOutput {
    let structured = serde_json::Value::Array(reports.iter().map(Report::structured).collect());
    let output = match reports {
        [only] => ToolOutput::text(
            DataSource::Agent {
                agent: only.agent.clone(),
            },
            only.text(),
        ),
        _ => {
            let body = reports
                .iter()
                .map(|report| {
                    UntrustedContent::new(
                        DataSource::Agent {
                            agent: report.agent.clone(),
                        },
                        report.text(),
                    )
                    .render()
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let agents = reports
                .iter()
                .map(|report| report.agent.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ToolOutput::text(DataSource::Agent { agent: agents }, body)
        }
    };
    output.with_structured(structured)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(agent: &str, state: DelegationState) -> Report {
        Report {
            state,
            ..Report::not_started(agent, "step budget of 4 exhausted")
        }
    }

    #[test]
    fn a_failed_child_reads_as_a_result_naming_why() {
        let text = report("researcher", DelegationState::Failed).text();
        assert_eq!(text, "researcher failed: step budget of 4 exhausted");
    }

    #[test]
    fn one_report_is_labelled_with_its_agent() {
        let mut done = report("researcher", DelegationState::Completed);
        done.result = Some("Churn was 4%.".into());
        let output = output(&[done]);
        assert_eq!(
            output.content.source,
            DataSource::Agent {
                agent: "researcher".into()
            }
        );
        assert_eq!(output.content.body, "Churn was 4%.");
        let structured = output.structured.unwrap();
        assert_eq!(structured[0]["state"], "completed");
        assert_eq!(structured[0]["agent"], "researcher");
    }

    #[test]
    fn several_reports_are_sealed_apart() {
        let output = output(&[
            report("researcher", DelegationState::Failed),
            report("summariser", DelegationState::Cancelled),
        ]);
        assert!(output.content.source.is_externally_influenced());
        assert!(output.content.body.contains("source=\"agent:researcher\""));
        assert!(output.content.body.contains("source=\"agent:summariser\""));
        assert_eq!(output.structured.unwrap().as_array().unwrap().len(), 2);
    }
}
