//! [`ExecutionReport`], and the builder that fills it in as a call proceeds.
//!
//! Every way a call can end, from an unknown tool to a success, leaves through
//! [`ReportBuilder`], so a refusal and a result have the same shape and a
//! refusal's text is labelled untrusted like any other tool output.

use std::time::Instant;

use agentos_core::Timestamp;
use agentos_core::ids::{ApprovalId, ToolExecutionId};
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::{ToolCall, ToolOutcome, ToolResult};
use agentos_core::trust::{DataSource, UntrustedContent};

use crate::error::ToolError;
use crate::tool::ToolPlan;

/// Everything that happened during one tool invocation.
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    /// Identity of this execution.
    pub execution_id: ToolExecutionId,
    /// The tool.
    pub tool: String,
    /// The provider's call identifier.
    pub call_id: String,
    /// Validated arguments, or the raw ones if validation failed.
    pub arguments: serde_json::Value,
    /// How it ended.
    pub outcome: ToolOutcome,
    /// The permission effect that applied.
    pub effect: Effect,
    /// Assessed risk.
    pub risk: RiskLevel,
    /// Whether the run was tainted at the time of the call.
    pub tainted: bool,
    /// The approval that gated it, if any.
    pub approval_id: Option<ApprovalId>,
    /// Bytes returned to the model.
    pub output_bytes: u64,
    /// Error text, when it failed.
    pub error: Option<String>,
    /// How long the whole pipeline took.
    pub duration_ms: u64,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished.
    pub completed_at: Timestamp,
    /// The plan, when one was produced.
    pub plan: Option<ToolPlan>,
    /// What goes back to the model.
    pub result: ToolResult,
}

impl ExecutionReport {
    /// Whether the tool actually ran and succeeded.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self.outcome, ToolOutcome::Success)
    }
}

pub(super) fn millis(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Accumulates the fields of an [`ExecutionReport`] as the pipeline progresses.
pub(super) struct ReportBuilder<'a> {
    pub(super) execution_id: ToolExecutionId,
    pub(super) call: &'a ToolCall,
    pub(super) started_at: Timestamp,
    pub(super) clock: Instant,
    pub(super) tainted: bool,
    pub(super) effect: Effect,
    pub(super) risk: RiskLevel,
    pub(super) approval_id: Option<ApprovalId>,
    pub(super) plan: Option<ToolPlan>,
    pub(super) arguments: serde_json::Value,
}

impl ReportBuilder<'_> {
    pub(super) fn failure(self, error: ToolError) -> ExecutionReport {
        let outcome = error.outcome();
        let message = error.to_string();
        // The refusal text is itself untrusted: it can embed a path or a URL the
        // model chose, and the model should not read its own strings back as
        // instructions.
        let content = UntrustedContent::new(
            DataSource::Tool {
                tool: self.call.tool.clone(),
            },
            &message,
        );
        let result = ToolResult {
            call_id: self.call.id.clone(),
            tool: self.call.tool.clone(),
            outcome,
            content,
            images: Vec::new(),
            structured: None,
        };
        self.finish(outcome, result, message.len(), Some(message))
    }

    pub(super) fn success(self, result: ToolResult, output_bytes: usize) -> ExecutionReport {
        self.finish(ToolOutcome::Success, result, output_bytes, None)
    }

    fn finish(
        self,
        outcome: ToolOutcome,
        result: ToolResult,
        output_bytes: usize,
        error: Option<String>,
    ) -> ExecutionReport {
        ExecutionReport {
            execution_id: self.execution_id,
            tool: self.call.tool.clone(),
            call_id: self.call.id.clone(),
            arguments: self.arguments,
            outcome,
            effect: self.effect,
            risk: self.risk,
            tainted: self.tainted,
            approval_id: self.approval_id,
            output_bytes: output_bytes as u64,
            error,
            duration_ms: millis(self.clock),
            started_at: self.started_at,
            completed_at: agentos_core::now(),
            plan: self.plan,
            result,
        }
    }
}
