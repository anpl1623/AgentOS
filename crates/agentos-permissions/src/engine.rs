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

use std::sync::Arc;

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

/// Several engines at once, where every one of them must agree.
///
/// A delegated run is evaluated by this, over its parent's engine and its own,
/// so it can do only what both policies permit: whatever its own policy grants,
/// a child cannot exceed its delegator, and whatever the delegator grants, a
/// child's own restrictions still hold. Nested delegation nests the
/// intersection, so a grandchild answers to every policy above it.
///
/// Each member evaluates the request against its own live policy and the
/// strictest effect wins — the fold the pipeline makes across a call's
/// capabilities. The alternative, merging two rule sets into one policy ahead
/// of time, would be a second policy evaluator that has to agree with the
/// first on every specificity tie and every ceiling; asking both engines needs
/// no such agreement.
///
/// The decision returned is the winning member's own, unaltered, so the rule
/// it names is the rule that actually refused or asked. Where members tie on
/// the effect, the one whose rules were stricter before taint escalation wins,
/// and otherwise the first: a card then says the policy asked when any member's
/// policy asked, and says taint alone is the reason only when it is the reason
/// for every member.
#[derive(Debug, Clone)]
pub struct IntersectionEngine {
    engines: Vec<Arc<dyn PermissionEngine>>,
}

impl IntersectionEngine {
    /// Intersect `engines`, the delegator's first.
    ///
    /// Order decides only which of two identical answers is reported. An empty
    /// list denies everything: the absence of a policy must never read as the
    /// absence of a restriction.
    #[must_use]
    pub fn new(engines: Vec<Arc<dyn PermissionEngine>>) -> Self {
        Self { engines }
    }

    /// The engines being intersected, in the order given.
    #[must_use]
    pub fn engines(&self) -> &[Arc<dyn PermissionEngine>] {
        &self.engines
    }
}

impl PermissionEngine for IntersectionEngine {
    fn evaluate(&self, request: &PermissionRequest) -> PermissionDecision {
        let mut winner: Option<PermissionDecision> = None;
        for engine in &self.engines {
            let decision = engine.evaluate(request);
            winner = match winner {
                Some(current) if !outranks(&decision, &current) => Some(current),
                _ => Some(decision),
            };
        }
        winner.unwrap_or_else(|| {
            PermissionDecision::new(Effect::Deny, "no policy was supplied")
                .with_rule("builtin:intersection-empty")
        })
    }
}

/// Whether `challenger` is strictly stricter than `incumbent`: by effect, and
/// on an equal effect by what the rules said before taint escalation.
fn outranks(challenger: &PermissionDecision, incumbent: &PermissionDecision) -> bool {
    let rank = |decision: &PermissionDecision| {
        (
            decision.effect.precedence(),
            decision.effect_before_taint.precedence(),
        )
    };
    rank(challenger) > rank(incumbent)
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

    /// An engine that gives one fixed answer, so a test can choose exactly
    /// what each member of an intersection says.
    #[derive(Debug)]
    struct Fixed(PermissionDecision);

    impl PermissionEngine for Fixed {
        fn evaluate(&self, _request: &PermissionRequest) -> PermissionDecision {
            self.0.clone()
        }
    }

    fn says(effect: Effect, rule: &str) -> Arc<dyn PermissionEngine> {
        Arc::new(Fixed(PermissionDecision::new(effect, rule).with_rule(rule)))
    }

    fn any_request() -> PermissionRequest {
        request("filesystem", "write", RiskLevel::Medium)
    }

    #[test]
    fn a_child_cannot_do_what_its_parent_denies() {
        let both = IntersectionEngine::new(vec![
            says(Effect::Deny, "parent-deny"),
            says(Effect::Allow, "child-allow"),
        ]);
        let decision = both.evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(decision.matched_rule.as_deref(), Some("parent-deny"));
    }

    #[test]
    fn a_parent_cannot_lift_what_the_child_denies() {
        let both = IntersectionEngine::new(vec![
            says(Effect::Allow, "parent-allow"),
            says(Effect::Deny, "child-deny"),
        ]);
        let decision = both.evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(decision.matched_rule.as_deref(), Some("child-deny"));
    }

    #[test]
    fn an_ask_on_either_side_asks() {
        for (parent, child, asker) in [
            (Effect::Ask, Effect::Allow, "parent"),
            (Effect::Allow, Effect::Ask, "child"),
        ] {
            let both = IntersectionEngine::new(vec![says(parent, "parent"), says(child, "child")]);
            let decision = both.evaluate(&any_request());
            assert_eq!(decision.effect, Effect::Ask);
            assert_eq!(decision.matched_rule.as_deref(), Some(asker));
        }
    }

    #[test]
    fn agreement_reports_the_first_engine() {
        let both = IntersectionEngine::new(vec![
            says(Effect::Allow, "parent-allow"),
            says(Effect::Allow, "child-allow"),
        ]);
        let decision = both.evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(decision.matched_rule.as_deref(), Some("parent-allow"));
    }

    #[test]
    fn an_empty_intersection_denies() {
        let decision = IntersectionEngine::new(Vec::new()).evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("builtin:intersection-empty")
        );
    }

    #[test]
    fn taint_is_named_as_the_reason_only_when_it_is_the_reason_for_every_policy() {
        // The parent's rules allow and only taint makes it ask; the child's
        // rules ask regardless. The card must not say that the rules would
        // have let this through silently, because the child's would not.
        let escalated: Arc<dyn PermissionEngine> = Arc::new(Fixed(
            PermissionDecision::new(Effect::Ask, "escalated")
                .with_rule("parent-allow")
                .with_effect_before_taint(Effect::Allow),
        ));
        let both = IntersectionEngine::new(vec![escalated.clone(), says(Effect::Ask, "child-ask")]);
        let decision = both.evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Ask);
        assert_eq!(decision.effect_before_taint, Effect::Ask);
        assert_eq!(decision.matched_rule.as_deref(), Some("child-ask"));
        assert!(!decision.was_escalated_by_taint());

        // When taint is what makes both of them ask, the card says so.
        let both = IntersectionEngine::new(vec![escalated.clone(), escalated]);
        assert!(both.evaluate(&any_request()).was_escalated_by_taint());
    }

    #[test]
    fn real_policies_intersect_through_their_own_rules() {
        // The supervisor may write anywhere under /work; the worker only
        // under /work/out. The worker's write elsewhere under /work is denied
        // by the worker's default, and its write outside /work by both.
        let supervisor = Policy::deny_all("supervisor").with_rule(
            PolicyRule::new("supervisor-write", "filesystem", "write", Effect::Allow)
                .with_resources(vec![ResourcePattern::path_prefix(PathBuf::from("/work"))]),
        );
        let worker = Policy::deny_all("worker").with_rule(
            PolicyRule::new("worker-write", "filesystem", "write", Effect::Allow).with_resources(
                vec![ResourcePattern::path_prefix(PathBuf::from("/work/out"))],
            ),
        );
        let both = IntersectionEngine::new(vec![
            Arc::new(PolicyEngine::new(supervisor)),
            Arc::new(PolicyEngine::new(worker)),
        ]);

        let inside = both.evaluate(&path_request("write", "/work/out/r.txt", RiskLevel::Medium));
        assert_eq!(inside.effect, Effect::Allow);
        assert_eq!(inside.matched_rule.as_deref(), Some("supervisor-write"));

        let beside = both.evaluate(&path_request("write", "/work/notes.txt", RiskLevel::Medium));
        assert_eq!(beside.effect, Effect::Deny);
        assert!(
            beside.reason.contains("policy default"),
            "{}",
            beside.reason
        );

        // Escalation happens inside each member, so a tainted run asks even
        // though neither policy's rules would have.
        let tainted = both
            .evaluate(&path_request("write", "/work/out/r.txt", RiskLevel::Medium).tainted(true));
        assert_eq!(tainted.effect, Effect::Ask);
        assert!(tainted.was_escalated_by_taint());
    }

    #[test]
    fn a_grandchild_answers_to_every_policy_above_it() {
        let parent_and_child = IntersectionEngine::new(vec![
            says(Effect::Allow, "root-allow"),
            says(Effect::Deny, "middle-deny"),
        ]);
        let grandchild = IntersectionEngine::new(vec![
            Arc::new(parent_and_child),
            says(Effect::Allow, "leaf-allow"),
        ]);
        let decision = grandchild.evaluate(&any_request());
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(decision.matched_rule.as_deref(), Some("middle-deny"));
    }

    #[test]
    fn no_policy_in_an_intersection_can_grant_self_modification() {
        let everything = || -> Arc<dyn PermissionEngine> {
            Arc::new(PolicyEngine::new(
                Policy::deny_all("p").with_rule(PolicyRule::new("all", "*", "*", Effect::Allow)),
            ))
        };
        let both = IntersectionEngine::new(vec![everything(), everything()]);
        let decision = both.evaluate(&request("runtime", "modify_policy", RiskLevel::Low));
        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("builtin:immutable-deny")
        );
    }
}
