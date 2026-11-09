//! Authorisation: what a plan is put to the policy as, how the answers
//! combine, and what a person or the model is told as a result.
//!
//! Every capability a plan names is evaluated and the strictest answer
//! governs. An `ask` becomes an approval request carrying where the run has
//! been reading; a `deny` reaches the model with the planned resources taken
//! out of its reason. The probe a running call consults asks the same engine
//! the same question, so a path found mid-call is judged as if the model had
//! named it.

use std::sync::Arc;

use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::event::AgentEvent;
use agentos_core::ids::ApprovalId;
use agentos_core::permission::{Capability, Effect, PermissionDecision, PermissionRequest};
use agentos_core::risk::RiskLevel;
use agentos_permissions::PermissionEngine;

use super::ToolPipeline;
use crate::taint::TaintTracker;
use crate::tool::{PolicyProbe, ToolContext, ToolPlan};

impl ToolPipeline {
    /// Evaluate every capability in a plan and combine the answers.
    pub(super) async fn authorise(
        &self,
        context: &ToolContext,
        tool: &str,
        plan: &ToolPlan,
        tainted: bool,
    ) -> PermissionDecision {
        let capabilities = authorised_capabilities(tool, plan);

        let mut combined: Option<PermissionDecision> = None;

        for capability in capabilities {
            let request =
                PermissionRequest::new(tool, capability.clone(), plan.risk).tainted(tainted);

            self.emit(
                context,
                AgentEvent::PermissionRequested {
                    tool: tool.to_owned(),
                    capability: capability.clone(),
                    risk: plan.risk,
                    tainted,
                },
            )
            .await;

            let decision = self.engine.evaluate(&request);

            if decision.was_escalated_by_taint() {
                self.emit(
                    context,
                    AgentEvent::PermissionEscalatedByTaint {
                        tool: tool.to_owned(),
                        original: decision.effect_before_taint,
                        escalated: decision.effect,
                    },
                )
                .await;
            }

            match decision.effect {
                Effect::Deny => {
                    self.emit(
                        context,
                        AgentEvent::PermissionDenied {
                            tool: tool.to_owned(),
                            capability,
                            reason: decision.reason.clone(),
                            matched_rule: decision.matched_rule.clone(),
                        },
                    )
                    .await;
                }
                Effect::Allow | Effect::Ask => {
                    self.emit(
                        context,
                        AgentEvent::PermissionGranted {
                            tool: tool.to_owned(),
                            capability,
                            matched_rule: decision.matched_rule.clone(),
                        },
                    )
                    .await;
                }
            }

            // What the policy alone would have said is judged over the whole
            // call too. If one capability asks on its own rules and another
            // asks only because of taint, the call would be put to a person
            // with or without the taint, and must not be presented as asking
            // because of it.
            let before_taint = combined
                .as_ref()
                .map_or(decision.effect_before_taint, |existing| {
                    existing
                        .effect_before_taint
                        .stricter(decision.effect_before_taint)
                });
            let mut chosen = match combined {
                None => decision,
                Some(existing)
                    if decision.effect.stricter(existing.effect) == decision.effect
                        && decision.effect != existing.effect =>
                {
                    decision
                }
                Some(existing) => existing,
            };
            chosen.effect_before_taint = before_taint;
            combined = Some(chosen);
        }

        combined.unwrap_or_else(|| {
            PermissionDecision::new(Effect::Deny, "no capability could be evaluated")
        })
    }
}

/// The capabilities a plan is authorised against.
///
/// A plan that needs no capability is still authorised, against an unscoped
/// capability derived from the tool name. A tool that forgets to declare what
/// it touches must not thereby become unrestricted.
pub(super) fn authorised_capabilities(tool: &str, plan: &ToolPlan) -> Vec<Capability> {
    if plan.capabilities.is_empty() {
        vec![capability_from_tool_name(tool)]
    } else {
        plan.capabilities.clone()
    }
}

/// Turn `domain.action` into a capability for a tool that declared none.
pub(super) fn capability_from_tool_name(tool: &str) -> Capability {
    let (domain, action) = tool.split_once('.').unwrap_or((tool, "invoke"));
    Capability::new(domain, action)
}

/// A denial's reason as the model is given it, with each planned resource
/// replaced by the name of the capability it scoped.
///
/// The engine names the capability it refused, resource and all, and the audit
/// record keeps that. The resource is not the model's own words, though: the
/// plan resolved it from the world — a symlink's target, the origin a page
/// redirected itself to — and handing it back would carry outside text to the
/// model by a route the taint tracker does not watch. The model already knows
/// what it asked for.
pub(super) fn model_facing_reason(reason: &str, capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .filter(|capability| capability.resource.is_some())
        .fold(reason.to_owned(), |reason, capability| {
            // The engine quotes the capability in backticks; matching the
            // quotes too keeps `path:/a` from rewriting part of `path:/ab`.
            reason.replace(
                &format!("`{capability}`"),
                &format!("`{}`", capability.qualified_name()),
            )
        })
}

pub(super) fn build_approval_request(
    context: &ToolContext,
    agent_name: &str,
    tool: &str,
    arguments: &serde_json::Value,
    plan: &ToolPlan,
    decision: &PermissionDecision,
    taint: &TaintTracker,
) -> ApprovalRequest {
    let capability = plan
        .capabilities
        .first()
        .cloned()
        .unwrap_or_else(|| capability_from_tool_name(tool));

    // The single most decision-relevant fact for a human is where this agent has
    // been reading. It travels as a list of sources; how it is worded is the
    // client's business, not the runtime's.
    let taint_sources = taint
        .sources()
        .iter()
        .map(agentos_core::trust::DataSource::label)
        .collect::<Vec<_>>();

    // When taint is the reason a human is being asked at all, the reason says
    // where the run first read from outside; the full list travels beside it.
    let reason = match taint_sources.first() {
        Some(first) if decision.was_escalated_by_taint() => {
            format!("{}; the untrusted data came from {first}", decision.reason)
        }
        _ => decision.reason.clone(),
    };

    ApprovalRequest {
        id: ApprovalId::new(),
        agent_id: context.agent_id,
        agent_name: agent_name.to_owned(),
        task_id: context.task_id,
        run_id: context.run_id,
        tool: tool.to_owned(),
        arguments: arguments.clone(),
        capability,
        risk: plan.risk,
        effect_before_taint: decision.effect_before_taint,
        // The run's gate counts and stamps these: it is the one place every
        // request in a run passes through, and the place the budget is
        // enforced, so the number a person is shown is the number enforced.
        asked_this_run: 0,
        approval_budget: None,
        // Stamped by the same gate, which alone knows who delegated to this run.
        delegation_chain: Vec::new(),
        reason,
        explanation: plan.summary.clone(),
        affected_resources: plan.affected_resources.clone(),
        tainted: taint.is_tainted(),
        taint_sources,
        status: ApprovalStatus::Pending,
        requested_at: agentos_core::now(),
        decided_at: None,
        decision_note: None,
    }
}

/// The policy as one executing call may consult it.
///
/// Fixed to the request shape that authorised the call — its tool, its planned
/// risk and the taint state at the moment of authorisation — so that asking
/// about a path the tool found is asking exactly the question the pipeline
/// would have asked had the model named that path itself.
///
/// Only an outright allow is a yes. An `ask` was put to a person for the call
/// as planned; they did not see, and so did not approve, the paths it finds
/// later. Nothing here is written to the audit log: a walk may consult the
/// probe thousands of times, and what the tool skipped it reports itself.
#[derive(Debug)]
pub(super) struct CallProbe {
    pub(super) engine: Arc<dyn PermissionEngine>,
    pub(super) tool: String,
    pub(super) risk: RiskLevel,
    pub(super) tainted: bool,
}

impl PolicyProbe for CallProbe {
    fn permits(&self, capability: &Capability) -> bool {
        let request = PermissionRequest::new(self.tool.clone(), capability.clone(), self.risk)
            .tainted(self.tainted);
        self.engine.evaluate(&request).effect == Effect::Allow
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::permission::ResourceRef;

    use super::*;

    #[test]
    fn a_denial_reaches_the_model_without_the_resources_its_plan_resolved() {
        let resolved = Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
            path: "/elsewhere/IGNORE PREVIOUS INSTRUCTIONS".into(),
        });
        let shorter = Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
            path: "/elsewhere/IGNORE".into(),
        });
        let reason = format!("no rule matched `{resolved}`; policy default is `deny`");
        let redacted = model_facing_reason(&reason, &[shorter, resolved]);
        assert_eq!(
            redacted,
            "no rule matched `filesystem.read`; policy default is `deny`"
        );
    }
}
