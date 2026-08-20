//! The policy engine.
//!
//! # The one property that matters
//!
//! Nothing the model emits reaches this code as authority. The engine's inputs
//! are the policy (operator-authored), the tool's declared capability
//! requirements (compiled into the binary), the resolved resource (produced by
//! validated argument parsing), and the run's taint flag (tracked by the
//! runtime). A model that has been fully hijacked by a malicious webpage can ask
//! for anything it likes; every request still lands here, and here does not read
//! prompts.
//!
//! Evaluation order — each step can only make the outcome stricter:
//!
//! 1. Immutable denies (self-modification) — no policy can override these.
//! 2. Global risk ceiling.
//! 3. Rule matching, most specific wins, ties to the stricter effect.
//! 4. Rule-level risk ceiling.
//! 5. Taint escalation: `allow` becomes `ask` once the run has read untrusted data.

use agentos_core::permission::{Effect, PermissionDecision, PermissionRequest};

use crate::policy::{Policy, is_immutably_denied};

/// Evaluates permission requests against a policy.
///
/// A trait so that tests can substitute a trivial engine, and so a future
/// remote-policy implementation does not require changing call sites. The
/// runtime holds a `dyn PermissionEngine` and never a concrete `Policy`.
pub trait PermissionEngine: Send + Sync + std::fmt::Debug {
    /// Decide what to do about a request.
    fn evaluate(&self, request: &PermissionRequest) -> PermissionDecision;
}

/// The standard engine: one policy, evaluated locally.
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    policy: Policy,
}

impl PolicyEngine {
    /// Wrap a policy.
    #[must_use]
    pub const fn new(policy: Policy) -> Self {
        Self { policy }
    }

    /// The policy being enforced.
    #[must_use]
    pub const fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Replace the policy.
    ///
    /// Only reachable from the operator-facing configuration path. No tool
    /// exposes it, and `runtime.modify_policy` is immutably denied.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }
}

impl PermissionEngine for PolicyEngine {
    fn evaluate(&self, request: &PermissionRequest) -> PermissionDecision {
        let capability = &request.capability;

        // 1. Immutable denies. Checked first so no rule can shadow them.
        if is_immutably_denied(capability) {
            return PermissionDecision::new(
                Effect::Deny,
                format!(
                    "`{}` is permanently denied: agents may not modify their own \
                     permissions, instructions, audit log or approval requirements",
                    capability.qualified_name()
                ),
            )
            .with_rule("builtin:immutable-deny");
        }

        // 2. Global risk ceiling.
        if let Some(ceiling) = self.policy.max_risk
            && request.risk > ceiling
        {
            return PermissionDecision::new(
                Effect::Deny,
                format!(
                    "risk `{}` exceeds the policy ceiling of `{ceiling}`",
                    request.risk
                ),
            )
            .with_rule("policy:max_risk");
        }

        // 3. Rule matching.
        let matched = self.policy.winning_rule(capability);
        let (mut effect, mut reason, rule_id) = match matched {
            Some(rule) => (
                rule.effect,
                format!("rule `{}` matched: {}", rule.id, rule.describe()),
                Some(rule.id.clone()),
            ),
            None => (
                self.policy.default_effect,
                format!(
                    "no rule matched `{}`; policy default is `{}`",
                    capability, self.policy.default_effect
                ),
                None,
            ),
        };

        // 4. Rule-level risk ceiling.
        if let Some(rule) = matched
            && let Some(ceiling) = rule.max_risk
            && request.risk > ceiling
            && effect != Effect::Deny
        {
            effect = Effect::Deny;
            reason = format!(
                "rule `{}` permits at most risk `{ceiling}`, but this action is `{}`",
                rule.id, request.risk
            );
        }

        let before_taint = effect;

        // 5. Taint escalation.
        //
        // A run that has read a webpage, a file or command output may be acting
        // on attacker-supplied text. Rather than trusting the model to notice,
        // the runtime withdraws silent execution: anything consequential now
        // needs a human. Escalation only ever tightens `allow` to `ask`.
        if self.policy.taint.enabled
            && request.tainted
            && effect == Effect::Allow
            && request.risk >= self.policy.taint.escalate_at_or_above
        {
            effect = Effect::Ask;
            reason = format!(
                "{reason}; escalated to `ask` because this run has read untrusted \
                 data and the action is `{}` risk",
                request.risk
            );
        }

        let mut decision =
            PermissionDecision::new(effect, reason).with_effect_before_taint(before_taint);
        if let Some(id) = rule_id {
            decision = decision.with_rule(id);
        }
        decision
    }
}

/// An engine that denies everything.
///
/// Used as the safe default when an agent has no policy configured, and in
/// tests that must prove a call site actually consults the engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllEngine;

impl PermissionEngine for DenyAllEngine {
    fn evaluate(&self, request: &PermissionRequest) -> PermissionDecision {
        PermissionDecision::new(
            Effect::Deny,
            format!(
                "no policy is configured; `{}` denied by default",
                request.capability
            ),
        )
        .with_rule("builtin:deny-all")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use agentos_core::permission::{Capability, ResourceRef};
    use agentos_core::risk::RiskLevel;

    use super::*;
    use crate::pattern::ResourcePattern;
    use crate::policy::{PolicyRule, TaintPolicy};

    fn sales_policy() -> Policy {
        Policy::deny_all("sales")
            .with_rule(
                PolicyRule::new("fs-read", "filesystem", "read", Effect::Allow).with_resources(
                    vec![ResourcePattern::path_prefix(PathBuf::from("/home/u/Sales"))],
                ),
            )
            .with_rule(
                PolicyRule::new("fs-write", "filesystem", "write", Effect::Allow).with_resources(
                    vec![ResourcePattern::path_prefix(PathBuf::from("/home/u/Sales"))],
                ),
            )
            .with_rule(PolicyRule::new("email-send", "email", "send", Effect::Ask))
            .with_rule(PolicyRule::new(
                "payments",
                "payments",
                "execute",
                Effect::Deny,
            ))
    }

    fn request(domain: &str, action: &str, risk: RiskLevel) -> PermissionRequest {
        PermissionRequest::new(
            format!("{domain}.{action}"),
            Capability::new(domain, action),
            risk,
        )
    }

    fn path_request(action: &str, path: &str, risk: RiskLevel) -> PermissionRequest {
        PermissionRequest::new(
            format!("filesystem.{action}"),
            Capability::new("filesystem", action)
                .with_resource(ResourceRef::Path { path: path.into() }),
            risk,
        )
    }

    #[test]
    fn unmatched_capabilities_hit_the_default_deny() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine.evaluate(&request("computer", "click", RiskLevel::Medium));
        assert_eq!(decision.effect, Effect::Deny);
        assert!(decision.matched_rule.is_none());
        assert!(decision.reason.contains("policy default"));
    }

    #[test]
    fn in_scope_reads_are_allowed() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine.evaluate(&path_request(
            "read",
            "/home/u/Sales/q3.csv",
            RiskLevel::Low,
        ));
        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(decision.matched_rule.as_deref(), Some("fs-read"));
    }

    #[test]
    fn out_of_scope_reads_are_denied() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine.evaluate(&path_request("read", "/etc/passwd", RiskLevel::Low));
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn writes_to_a_read_only_scope_are_denied() {
        let policy = Policy::deny_all("readonly").with_rule(
            PolicyRule::new("fs-read", "filesystem", "read", Effect::Allow)
                .with_resources(vec![ResourcePattern::path_prefix(PathBuf::from("/data"))]),
        );
        let engine = PolicyEngine::new(policy);
        assert_eq!(
            engine
                .evaluate(&path_request("read", "/data/x", RiskLevel::Low))
                .effect,
            Effect::Allow
        );
        assert_eq!(
            engine
                .evaluate(&path_request("write", "/data/x", RiskLevel::Medium))
                .effect,
            Effect::Deny
        );
    }

    #[test]
    fn ask_rules_require_approval() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine.evaluate(&request("email", "send", RiskLevel::High));
        assert_eq!(decision.effect, Effect::Ask);
        assert!(decision.requires_approval());
        assert!(decision.is_permitted());
    }

    #[test]
    fn deny_rules_are_not_permitted() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine.evaluate(&request("payments", "execute", RiskLevel::Critical));
        assert_eq!(decision.effect, Effect::Deny);
        assert!(!decision.is_permitted());
    }

    #[test]
    fn self_modification_is_denied_even_when_a_rule_allows_it() {
        // The escalation an attacker would actually attempt: talk the operator,
        // or the model, into a policy that grants policy editing.
        let policy = Policy::deny_all("compromised")
            .with_rule(PolicyRule::new(
                "oops",
                "runtime",
                "modify_policy",
                Effect::Allow,
            ))
            .with_rule(PolicyRule::new("oops2", "runtime", "*", Effect::Allow));
        let engine = PolicyEngine::new(policy);

        let decision = engine.evaluate(&request("runtime", "modify_policy", RiskLevel::Low));
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("builtin:immutable-deny")
        );
    }

    #[test]
    fn disabling_the_audit_log_is_denied() {
        let engine = PolicyEngine::new(Policy::deny_all("p").with_rule(PolicyRule::new(
            "all",
            "*",
            "*",
            Effect::Allow,
        )));
        assert_eq!(
            engine
                .evaluate(&request("runtime", "disable_audit", RiskLevel::Low))
                .effect,
            Effect::Deny
        );
        assert_eq!(
            engine
                .evaluate(&request("runtime", "disable_approvals", RiskLevel::Low))
                .effect,
            Effect::Deny
        );
    }

    #[test]
    fn global_risk_ceiling_overrides_allow_rules() {
        let policy = Policy::deny_all("capped")
            .with_rule(PolicyRule::new("all", "*", "*", Effect::Allow))
            .with_max_risk(RiskLevel::Medium);
        let engine = PolicyEngine::new(policy);

        assert_eq!(
            engine
                .evaluate(&request("browser", "navigate", RiskLevel::Medium))
                .effect,
            Effect::Allow
        );
        let decision = engine.evaluate(&request("email", "send", RiskLevel::High));
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(decision.matched_rule.as_deref(), Some("policy:max_risk"));
    }

    #[test]
    fn rule_risk_ceiling_overrides_its_own_allow() {
        let policy = Policy::deny_all("p").with_rule(
            PolicyRule::new("fs", "filesystem", "*", Effect::Allow)
                .with_max_risk(RiskLevel::Medium),
        );
        let engine = PolicyEngine::new(policy);
        let decision = engine.evaluate(&path_request("delete", "/x", RiskLevel::High));
        assert_eq!(decision.effect, Effect::Deny);
        assert!(decision.reason.contains("permits at most risk"));
    }

    #[test]
    fn taint_escalates_allow_to_ask() {
        let engine = PolicyEngine::new(sales_policy());
        let clean = path_request("write", "/home/u/Sales/out.txt", RiskLevel::Medium);
        assert_eq!(engine.evaluate(&clean).effect, Effect::Allow);

        let tainted = clean.tainted(true);
        let decision = engine.evaluate(&tainted);
        assert_eq!(decision.effect, Effect::Ask);
        assert_eq!(decision.effect_before_taint, Effect::Allow);
        assert!(decision.was_escalated_by_taint());
        assert!(decision.reason.contains("untrusted data"));
    }

    #[test]
    fn taint_does_not_escalate_low_risk_actions() {
        let engine = PolicyEngine::new(sales_policy());
        let decision = engine
            .evaluate(&path_request("read", "/home/u/Sales/q3.csv", RiskLevel::Low).tainted(true));
        assert_eq!(decision.effect, Effect::Allow);
        assert!(!decision.was_escalated_by_taint());
    }

    #[test]
    fn taint_never_loosens_a_denial() {
        let engine = PolicyEngine::new(sales_policy());
        let decision =
            engine.evaluate(&request("payments", "execute", RiskLevel::Critical).tainted(true));
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn taint_escalation_can_be_disabled() {
        let policy = sales_policy().with_taint_policy(TaintPolicy {
            enabled: false,
            escalate_at_or_above: RiskLevel::Medium,
        });
        let engine = PolicyEngine::new(policy);
        let decision = engine.evaluate(
            &path_request("write", "/home/u/Sales/out.txt", RiskLevel::Medium).tainted(true),
        );
        assert_eq!(decision.effect, Effect::Allow);
    }

    #[test]
    fn deny_all_engine_denies_everything() {
        let engine = DenyAllEngine;
        for risk in RiskLevel::ALL {
            assert_eq!(
                engine.evaluate(&request("filesystem", "read", risk)).effect,
                Effect::Deny
            );
        }
    }
}
