//! `agent.delegate` itself: its arguments, its plan and the call.
//!
//! Every refusal is decided here while the call is planned, before
//! authorisation and before any side effect. Once authorised, the call takes
//! its slots and starts each child, which records and drives itself.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use agentos_core::agent::Agent;
use agentos_core::delegation::DelegationRefusal;
use agentos_core::ids::TaskRunId;
use agentos_core::permission::{Capability, ResourceRef, permission_domains};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use agentos_tools::{
    Tool, ToolContext, ToolError, ToolOutput, ToolPlan, metadata_for, parse_arguments,
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::broker::{DelegationBroker, ParentRunHandle};
use super::child::{Child, output, start};
use super::{DELEGATE, TOOL_NAME};

/// How much of an objective the one-line summary on an approval card quotes.
const SUMMARY_OBJECTIVE_CHARS: usize = 80;

/// Arguments to `agent.delegate`.
///
/// A list rather than one request, so that fanning out and gathering the
/// results back in happen inside one parent run, where taint, approvals and
/// cancellation already reach. Work handed to the scheduler instead would run
/// later, unattended, and clean of everything the parent had read.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// The agents to ask, and what to ask each. They work at the same time,
    /// and each one's report comes back in this call's result.
    pub requests: Vec<DelegateRequest>,
}

/// One agent asked to carry out one objective.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateRequest {
    /// The name of the agent to ask.
    pub agent: String,
    /// What it should do, stated completely: it sees nothing of this
    /// conversation but this.
    pub objective: String,
}

/// Asks other agents to carry out objectives, and reports what they did.
#[derive(Debug)]
pub struct DelegateTool {
    broker: Arc<DelegationBroker>,
    metadata: ToolMetadata,
}

impl DelegateTool {
    /// The tool, performing delegations for the runs `broker` holds.
    #[must_use]
    pub fn new(broker: Arc<DelegationBroker>) -> Self {
        Self {
            broker,
            metadata: metadata_for::<DelegateArgs>(
                TOOL_NAME,
                "Ask other agents to carry out objectives and return their reports. Each agent \
                 works under its own policy and yours together, shares your step budget, and \
                 reports back when it finishes or fails.",
                // It causes another agent to act in the world.
                RiskLevel::High,
                vec![Capability::new(permission_domains::RUNTIME, DELEGATE)],
                true,
            ),
        }
    }

    /// The handle for the run a call belongs to.
    fn handle_for(&self, context: &ToolContext) -> Result<Arc<ParentRunHandle>, ToolError> {
        // Every run the runtime prepares is registered, so a call from one
        // that is not is a fault in the runtime, not a permitted call.
        self.broker.handle(context.run_id).ok_or_else(|| {
            ToolError::Failed(
                "this run cannot delegate: the runtime did not register it with this tool"
                    .to_owned(),
            )
        })
    }

    /// Whether a run may make these requests at all, before looking at whom
    /// they ask.
    fn refuse_shape(
        &self,
        handle: &ParentRunHandle,
        requests: &[DelegateRequest],
    ) -> Option<DelegationRefusal> {
        let limits = self.broker.limits();
        let fan_out = DelegationRefusal::FanOutExceeded {
            limit: limits.max_children_per_run,
        };
        let wanted = u16::try_from(requests.len()).unwrap_or(u16::MAX);
        if requests.is_empty() || wanted > limits.max_children_per_run {
            return Some(fan_out);
        }
        let started = handle.children_started.load(Ordering::SeqCst);
        if started.saturating_add(wanted) > limits.max_children_per_run {
            return Some(fan_out);
        }
        if handle.depth >= limits.max_depth {
            return Some(DelegationRefusal::DepthExceeded {
                limit: limits.max_depth,
            });
        }
        let remaining = handle.budget.remaining();
        if remaining == 0 {
            return Some(DelegationRefusal::StepBudgetExhausted { remaining });
        }
        None
    }

    /// Plan the call, recording any refusal in the parent's trail.
    async fn plan_for(
        &self,
        handle: &ParentRunHandle,
        requests: &[DelegateRequest],
    ) -> Result<ToolPlan, ToolError> {
        if let Some(refusal) = self.refuse_shape(handle, requests) {
            let asked = requests
                .iter()
                .map(|request| request.agent.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            handle.record_refusal(&asked, &refusal).await;
            return Err(denied(&refusal));
        }

        let mut summaries = Vec::with_capacity(requests.len());
        let mut capabilities = Vec::with_capacity(requests.len());
        for request in requests {
            let child = match vet(handle, &request.agent).await {
                Ok(child) => child,
                Err(Vetted::Refused(refusal)) => {
                    handle.record_refusal(&request.agent, &refusal).await;
                    return Err(denied(&refusal));
                }
                Err(Vetted::Unreadable(error)) => {
                    return Err(ToolError::Failed(format!(
                        "the agents could not be read: {error}"
                    )));
                }
            };
            summaries.push(format!(
                "Ask {} to: {}",
                child.name,
                shorten(&request.objective, SUMMARY_OBJECTIVE_CHARS)
            ));
            capabilities.push(
                Capability::new(permission_domains::RUNTIME, DELEGATE)
                    .with_resource(ResourceRef::Named { name: child.name }),
            );
        }

        // One capability per child, so the policy is asked about each, and
        // the strictest answer governs the whole call.
        Ok(capabilities.into_iter().fold(
            ToolPlan::new(RiskLevel::High, summaries.join("; ")),
            ToolPlan::requiring,
        ))
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: DelegateArgs = parse_arguments(TOOL_NAME, arguments)?;
        Ok(arguments.clone())
    }

    /// Decide every refusal, before authorisation and before any side effect.
    ///
    /// Reading the agents table is looking at the world, which planning may
    /// do; the only thing written is the audit record of a refusal.
    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: DelegateArgs = parse_arguments(TOOL_NAME, arguments)?;
        let handle = self.handle_for(context)?;
        self.plan_for(&handle, &args.requests).await
    }

    /// None. A delegation lasts as long as its children's runs, each of
    /// whose model requests and tool calls is limited on its own and any of
    /// which may wait on a person; the shared step budget bounds how much
    /// work there is, and the parent's token stops it.
    fn time_limit(&self, _context: &ToolContext) -> Option<std::time::Duration> {
        None
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: DelegateArgs = parse_arguments(TOOL_NAME, &arguments)?;
        let handle = self.handle_for(context)?;

        // The plan checked the count; this takes the slots, atomically, so
        // that nothing between the plan and here can spend them twice.
        let limit = self.broker.limits().max_children_per_run;
        let wanted = u16::try_from(args.requests.len()).unwrap_or(u16::MAX);
        #[allow(
            deprecated,
            reason = "fetch_update's new name, try_update, is stable from Rust 1.95; rust-version is 1.94"
        )]
        let exceeded = handle
            .children_started
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |started| {
                started.checked_add(wanted).filter(|total| *total <= limit)
            })
            .is_err();
        if exceeded {
            return Err(denied(&DelegationRefusal::FanOutExceeded { limit }));
        }

        let mut children = Vec::with_capacity(args.requests.len());
        let mut guards = Vec::with_capacity(args.requests.len());
        for request in args.requests {
            if cancel.is_cancelled() {
                break;
            }
            let child = start(&handle, request).await;
            if let Child::Started { cancel, .. } = &child {
                // This call has no time limit, but it can still be dropped
                // before it returns, as the parent's run is torn down. Each
                // child is driven on a task of its own and records its own
                // ending, so dropping the call cannot orphan its bookkeeping;
                // these make sure it stops the children too, rather than
                // leaving them working for a parent that has gone.
                guards.push(cancel.clone().drop_guard());
            }
            children.push(child);
        }

        let mut reports = Vec::with_capacity(children.len());
        for child in children {
            reports.push(child.finish().await);
        }
        for guard in guards {
            guard.disarm();
        }

        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(output(&reports))
    }

    async fn end_run(&self, run_id: TaskRunId) {
        self.broker.forget(run_id);
    }
}

/// Why an agent could not be asked.
pub(super) enum Vetted {
    /// It may not be asked.
    Refused(DelegationRefusal),
    /// The agents could not be read.
    Unreadable(agentos_persistence::DbError),
}

/// Resolve an agent a run asked for, refusing one it may not ask.
pub(super) async fn vet(handle: &ParentRunHandle, name: &str) -> Result<Agent, Vetted> {
    let child = handle
        .runtime
        .database()
        .agents()
        .find_by_name(name)
        .await
        .map_err(Vetted::Unreadable)?
        .ok_or_else(|| {
            Vetted::Refused(DelegationRefusal::UnknownAgent {
                name: name.to_owned(),
            })
        })?;
    if !child.is_enabled() {
        return Err(Vetted::Refused(DelegationRefusal::DisabledAgent {
            name: child.name,
        }));
    }
    // The run's own agent is the last of its ancestry, so asking oneself is
    // the shortest cycle and is caught here with the rest.
    if handle.ancestry.contains(&child.id) {
        let mut path = handle.chain.clone();
        path.push(child.name);
        return Err(Vetted::Refused(DelegationRefusal::AgencyCycle { path }));
    }
    Ok(child)
}

/// The refusal as the model reads it.
fn denied(refusal: &DelegationRefusal) -> ToolError {
    ToolError::Denied {
        reason: refusal.to_string(),
    }
}

/// At most `max` characters of `text`, marked when cut.
fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        None => text.to_owned(),
        Some((cut, _)) => format!("{}…", &text[..cut]),
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::ids::{AgentId, TaskId};

    use super::*;
    use crate::config::DelegationLimits;

    #[test]
    fn a_summary_quotes_a_long_objective_only_in_part() {
        let long = "x".repeat(200);
        let short = shorten(&long, SUMMARY_OBJECTIVE_CHARS);
        assert_eq!(short.chars().count(), SUMMARY_OBJECTIVE_CHARS + 1);
        assert_eq!(shorten("  brief  ", SUMMARY_OBJECTIVE_CHARS), "brief");
    }

    #[test]
    fn the_tool_declares_one_capability_and_untrusted_output() {
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        let metadata = tool.metadata();
        assert_eq!(metadata.name, TOOL_NAME);
        assert_eq!(metadata.risk, RiskLevel::High);
        assert!(metadata.returns_untrusted_data);
        assert_eq!(metadata.capability_names(), vec!["runtime.delegate"]);
    }

    #[test]
    fn a_delegation_is_not_cut_off_at_the_per_call_timeout() {
        // A supervisor's call lasts as long as its workers do, which is
        // routinely longer than any one tool call is allowed.
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        let context = ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            std::env::temp_dir(),
        );
        assert_eq!(tool.time_limit(&context), None);
    }

    #[test]
    fn arguments_carry_nothing_the_tool_does_not_expect() {
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        assert!(
            tool.validate(&serde_json::json!({
                "requests": [{"agent": "worker", "objective": "o"}]
            }))
            .is_ok()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "requests": [{"agent": "worker", "objective": "o", "policy": "allow"}]
            }))
            .is_err()
        );
    }
}
