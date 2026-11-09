//! The tool execution pipeline.
//!
//! Every tool call the model makes goes through here, in a fixed order:
//!
//! ```text
//! is the tool known and enabled?
//!         ↓
//! validate arguments against the schema
//!         ↓
//! plan: what capabilities, what risk, what resources
//!         ↓
//! compare the plan with the tool's manifest     ← recorded, never enforced
//!         ↓
//! evaluate each capability against the policy   ← the model has no say here
//!         ↓
//! deny        → return a refusal to the model
//! ask         → put an approval to a human, wait
//! allow       → proceed
//!         ↓
//! execute with a timeout, a cancellation token, a policy probe and only
//! the credentials the plan named
//!         ↓
//! redact any credential the run released from what came back
//!         ↓
//! capture output as untrusted, raise taint from its provenance
//!         ↓
//! emit audit events at every step, return a report
//! ```
//!
//! Two properties are worth stating plainly:
//!
//! * **Nothing executes before authorisation.** `plan` is pure and side-effect
//!   free; the first side effect a tool has is inside `execute`, which is only
//!   reached after the policy engine and, where required, a human.
//! * **A refusal is a result, not a crash.** Denials come back to the model as
//!   ordinary tool output so it can re-plan. What it must never get is the
//!   ability to argue its way past the decision, and it cannot: the decision was
//!   made from the policy, not from anything the model wrote.
//!
//! A third follows from the same reasoning applied to tools: **what a tool says
//! about itself is a claim.** Its manifest is checked against each plan and any
//! excess is recorded. Taint comes from the declared provenance of the bytes a
//! call returned, and a call that may read the outside world taints the run
//! even if the tool labels its output as the runtime's own. What counts as
//! reading fails closed: a capability is presumed to read unless it is one of
//! the actions known only to change something.
//!
//! Credentials follow the same rule. A tool is never handed the run's
//! credential store, only one that releases what the authorised plan named and
//! writes down every value it releases. The pipeline reads that ledger, not the
//! tool's account of itself, to record each credential spent and to strip each
//! one from whatever the call hands back, success or failure, before the model
//! or the audit log sees it.

mod authorise;
mod credentials;
mod execute;
mod provenance;
mod report;
#[cfg(test)]
mod test_support;

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::event::{AgentEvent, Event};
use agentos_permissions::PermissionEngine;

use crate::approval::ApprovalGate;
use crate::tool::{ToolContext, ToolRegistry};

pub use report::ExecutionReport;

/// Runs tool calls through validation, authorisation, approval and execution.
#[derive(Debug, Clone)]
pub struct ToolPipeline {
    registry: Arc<ToolRegistry>,
    engine: Arc<dyn PermissionEngine>,
    approvals: Arc<dyn ApprovalGate>,
    audit: Arc<AuditLog>,
}

impl ToolPipeline {
    /// Assemble a pipeline.
    #[must_use]
    pub const fn new(
        registry: Arc<ToolRegistry>,
        engine: Arc<dyn PermissionEngine>,
        approvals: Arc<dyn ApprovalGate>,
        audit: Arc<AuditLog>,
    ) -> Self {
        Self {
            registry,
            engine,
            approvals,
            audit,
        }
    }

    /// The registry this pipeline draws from.
    #[must_use]
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    async fn emit(&self, context: &ToolContext, payload: AgentEvent) {
        let event = Event::new(payload)
            .for_agent(context.agent_id)
            .for_task(context.task_id)
            .for_run(context.run_id);
        if let Err(error) = self.audit.record(event).await {
            // Losing an audit record is serious but must not abort the run: the
            // alternative is that a failing disk silently stops all work. Nor
            // is it only this log line: the log has already counted the loss
            // in `agentos_audit::unrecorded`, which the desktop reports beside
            // the chain's health, so a chain that verifies is not mistaken for
            // a complete one.
            tracing::error!(%error, "failed to record audit event");
        }
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::tool::ToolCall;
    use tokio_util::sync::CancellationToken;

    use super::test_support::FullDisk;
    use super::*;
    use crate::taint::TaintTracker;

    #[tokio::test]
    async fn a_record_the_disk_refuses_is_counted_and_the_call_still_answers() {
        let audit = Arc::new(AuditLog::open(Arc::new(FullDisk)).await.unwrap());
        let pipeline = ToolPipeline::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(agentos_permissions::DenyAllEngine),
            Arc::new(crate::DenyAllGate),
            audit,
        );
        let context = ToolContext::new(
            agentos_core::ids::AgentId::new(),
            agentos_core::ids::TaskId::new(),
            agentos_core::ids::TaskRunId::new(),
            std::env::temp_dir(),
        );
        let before = agentos_audit::unrecorded();

        // An unknown tool is refused, and the refusal is a record the pipeline
        // tries and fails to write.
        let report = pipeline
            .execute(
                &ToolCall::new("1", "nowhere.nothing", serde_json::json!({})),
                &context,
                &TaintTracker::new(),
                "agent",
                &[],
                &CancellationToken::new(),
            )
            .await;

        assert!(
            !report.outcome.executed(),
            "the call was answered, as refused"
        );
        assert!(agentos_audit::unrecorded() > before, "the loss was counted");
    }
}
