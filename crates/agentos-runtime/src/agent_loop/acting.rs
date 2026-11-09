//! The states after a plan: running the tools it asked for, judging whether
//! the objective has been met, and recovering from a turn that failed.

use agentos_core::task::{TaskStepKind, TaskTrigger};
use agentos_persistence::ToolExecutionRecord;
use agentos_providers::{CompletionResponse, messages_for_tool_result};

use super::{AgentLoop, PROVIDER_RETRIES};
use crate::error::RuntimeError;

impl AgentLoop {
    /// Run the tools the model asked for.
    pub(super) async fn execute(&mut self) -> Result<TaskTrigger, RuntimeError> {
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
            let vision = self.provider.capabilities().vision;
            self.conversation
                .extend(messages_for_tool_result(&report.result, vision));
            if vision {
                self.forget_stale_images();
            }

            self.sync_taint().await?;

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
    pub(super) fn verify(&self) -> TaskTrigger {
        match self.last_response.as_ref() {
            Some(response) if !response.wants_tools() => TaskTrigger::VerificationPassed,
            Some(_) => TaskTrigger::VerificationNeedsMoreWork,
            None => TaskTrigger::VerificationNeedsMoreWork,
        }
    }

    pub(super) async fn recover(&mut self) -> TaskTrigger {
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
}
