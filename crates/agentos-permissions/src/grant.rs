//! What a policy grants, read from the policy rather than inferred from text.
//!
//! An operator deciding whether an agent can be trusted with a task needs to
//! know, for each tool it has been given, whether the policy lets that tool do
//! anything at all. The tool list says what the agent may ask for; the policy
//! says what asking gets it. Showing the first as though it were the second is
//! how a screen tells somebody an agent can send email when every rule that
//! mentions email denies it.
//!
//! So the answer is computed from the compiled [`Policy`] by the same pieces
//! the engine decides with: the immutable denies, the risk ceilings, the rules'
//! name patterns and their specificity, and each rule's effect. A `deny` rule
//! is never a grant, however specifically it names a capability.

use agentos_core::permission::{Capability, Effect};
use agentos_core::risk::RiskLevel;
use serde::{Deserialize, Serialize};

use crate::policy::{Policy, PolicyRule, is_immutably_denied};

/// How far a policy lets a capability reach.
///
/// Ordered from most to least: a caller showing the weakest of several can take
/// the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// Allowed without asking, for every resource.
    Allowed,
    /// Allowed, or put to a person, for some resources and not others: a rule
    /// scoped to paths, origins or programs, or a carve-out from a broader one.
    Scoped,
    /// Never allowed without asking, and asked for every resource.
    Asks,
    /// No rule can allow it or ask about it.
    Denied,
}

impl Reach {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Scoped => "scoped",
            Self::Asks => "asks",
            Self::Denied => "denied",
        }
    }
}

impl Policy {
    /// How far this policy lets `capability` reach, for an action of at least
    /// `risk`.
    ///
    /// `capability` is read for its domain and action; any resource on it is
    /// ignored, because the question is about every resource at once. `risk`
    /// is the tool's baseline: a call may raise its risk but never lower it, so
    /// a ceiling below the baseline denies every call the tool can make.
    ///
    /// The answer is for a run that has not read untrusted data. Taint can only
    /// turn an `allow` into an `ask`, never the reverse, so an untainted reach
    /// is the most a run can get.
    ///
    /// A rule scoped to some resources counts if it could win for any of them:
    /// if it outranks the rule that governs everywhere else.
    ///
    /// # What it overstates
    ///
    /// The answer can overstate a grant and never understates one: wherever
    /// the engine allows a call it says at least `Scoped`, and wherever the
    /// engine asks it says something other than `Denied`. It is read from the
    /// policy and a capability's name, which is not all the engine knows when
    /// it decides a call, so in these shapes it reports more than any call
    /// gets:
    ///
    /// - A scoped rule wholly covered by another, stricter one is still
    ///   counted.
    /// - Risk is read at the tool's baseline, and a call may plan higher. A
    ///   ceiling, the policy's or a rule's, between the baseline and the
    ///   call's risk denies that call, so `Allowed` can stand beside a class
    ///   of calls that is always refused: an overwrite where only new files
    ///   are permitted, a click that commits where only clicks are.
    /// - A scoped rule counts whatever kind of resource it names, so a rule
    ///   whose patterns can never match what the capability carries, origins
    ///   on a filesystem rule, or any resource on a capability a tool plans
    ///   with none, still reads as `Scoped`.
    ///
    /// Each would need the manifest to say more than it does: the highest
    /// risk a tool can plan, and the kind of resource each capability
    /// carries. Until it does, a reader should take `Allowed` as "allowed at
    /// the tool's usual risk" and `Scoped` as "possibly allowed somewhere".
    /// Overstating is the direction chosen: a report that told an operator an
    /// agent could not do something it can would be the dangerous error.
    #[must_use]
    pub fn reach(&self, capability: &Capability, risk: RiskLevel) -> Reach {
        let unscoped = Capability::new(capability.domain.clone(), capability.action.clone());
        if is_immutably_denied(&unscoped) || self.max_risk.is_some_and(|ceiling| risk > ceiling) {
            return Reach::Denied;
        }

        // What a resource no scoped rule names gets: the best rule that applies
        // to every resource, or the policy's default. This is also exactly what
        // the engine decides for a request that carries no resource.
        let everywhere = self.winning_rule(&unscoped);
        let base = everywhere.map_or(self.default_effect, |rule| effective(rule, risk));

        let mut elsewhere = Vec::new();
        for rule in &self.rules {
            if rule.resources.is_empty()
                || !rule.domain.matches(&unscoped.domain)
                || !rule.action.matches(&unscoped.action)
            {
                continue;
            }
            // A rule with `*` among its resources applies everywhere, and was
            // weighed above.
            if rule.matches(&unscoped) {
                continue;
            }
            if everywhere.is_none_or(|governing| outranks(rule, governing)) {
                elsewhere.push(effective(rule, risk));
            }
        }

        let grants = |effect: Effect| effect != Effect::Deny;
        if !grants(base) && !elsewhere.iter().copied().any(grants) {
            Reach::Denied
        } else if elsewhere.iter().all(|effect| *effect == base) {
            match base {
                Effect::Allow => Reach::Allowed,
                Effect::Ask => Reach::Asks,
                // Unreachable: a base of deny with nothing else granting was
                // answered above. Denied is the safe answer if that changes.
                Effect::Deny => Reach::Denied,
            }
        } else {
            Reach::Scoped
        }
    }
}

/// A rule's effect for an action of `risk`, after its own ceiling.
fn effective(rule: &PolicyRule, risk: RiskLevel) -> Effect {
    if rule.max_risk.is_some_and(|ceiling| risk > ceiling) {
        Effect::Deny
    } else {
        rule.effect
    }
}

/// Whether the scoped `rule` would beat `governing` for a resource it names.
///
/// The engine compares domain, then action, then resource specificity, and
/// gives a tie to the stricter effect. A scoped rule's resource specificity is
/// always above zero and an unscoped rule's is zero, so the scoped rule wins
/// exactly when its name patterns are at least as specific.
fn outranks(rule: &PolicyRule, governing: &PolicyRule) -> bool {
    (rule.domain.specificity(), rule.action.specificity())
        >= (
            governing.domain.specificity(),
            governing.action.specificity(),
        )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::pattern::{GlobKind, ResourcePattern};
    use crate::yaml::PolicyDocument;

    fn capability(name: &str) -> Capability {
        let (domain, action) = name.split_once('.').unwrap();
        Capability::new(domain, action)
    }

    fn compiled(yaml: &str) -> Policy {
        PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap()
    }

    #[test]
    fn with_no_rule_a_capability_is_denied() {
        let policy = Policy::deny_all("p");
        assert_eq!(
            policy.reach(&capability("email.send"), RiskLevel::High),
            Reach::Denied
        );
    }

    #[test]
    fn a_deny_rule_is_not_a_grant() {
        // The rule names the capability as specifically as a rule can. Naming
        // it is not granting it.
        let policy = Policy::deny_all("p").with_rule(PolicyRule::new(
            "no-email",
            "email",
            "send",
            Effect::Deny,
        ));
        assert_eq!(
            policy.reach(&capability("email.send"), RiskLevel::High),
            Reach::Denied
        );

        // Nor is a deny scoped to some paths, under a default of deny.
        let policy = Policy::deny_all("p").with_rule(
            PolicyRule::new("no-ssh", "filesystem", "read", Effect::Deny).with_resources(vec![
                ResourcePattern::path_prefix(PathBuf::from("/home/u/.ssh")),
            ]),
        );
        assert_eq!(
            policy.reach(&capability("filesystem.read"), RiskLevel::Low),
            Reach::Denied
        );
    }

    #[test]
    fn an_unscoped_allow_is_allowed_and_an_unscoped_ask_asks() {
        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new("read", "filesystem", "read", Effect::Allow))
            .with_rule(PolicyRule::new("send", "email", "send", Effect::Ask));
        assert_eq!(
            policy.reach(&capability("filesystem.read"), RiskLevel::Low),
            Reach::Allowed
        );
        assert_eq!(
            policy.reach(&capability("email.send"), RiskLevel::High),
            Reach::Asks
        );
    }

    #[test]
    fn a_path_scoped_allow_is_scoped() {
        let policy = compiled("default: deny\npermissions:\n  filesystem:\n    read: [\"/tmp\"]\n");
        assert_eq!(
            policy.reach(&capability("filesystem.read"), RiskLevel::Low),
            Reach::Scoped
        );
        // A capability the document does not mention is still denied.
        assert_eq!(
            policy.reach(&capability("filesystem.write"), RiskLevel::Medium),
            Reach::Denied
        );
    }

    #[test]
    fn an_origin_scoped_allow_is_scoped() {
        let policy = Policy::deny_all("p").with_rule(
            PolicyRule::new("crm", "browser", "navigate", Effect::Allow).with_resources(vec![
                ResourcePattern::glob(GlobKind::Origin, "https://crm.example.com").unwrap(),
            ]),
        );
        assert_eq!(
            policy.reach(&capability("browser.navigate"), RiskLevel::Medium),
            Reach::Scoped
        );
    }

    #[test]
    fn a_carve_out_from_a_broad_allow_is_scoped() {
        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new("read", "filesystem", "read", Effect::Allow))
            .with_rule(
                PolicyRule::new("no-ssh", "filesystem", "read", Effect::Deny).with_resources(vec![
                    ResourcePattern::path_prefix(PathBuf::from("/home/u/.ssh")),
                ]),
            );
        assert_eq!(
            policy.reach(&capability("filesystem.read"), RiskLevel::Low),
            Reach::Scoped
        );
    }

    #[test]
    fn a_scoped_rule_that_cannot_win_does_not_count() {
        // The engine compares the action before the resource, so a deny on
        // `filesystem.read` everywhere beats an allow on `filesystem.*` scoped
        // to one directory, wherever the request points.
        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new(
                "no-read",
                "filesystem",
                "read",
                Effect::Deny,
            ))
            .with_rule(
                PolicyRule::new("tmp", "filesystem", "*", Effect::Allow)
                    .with_resources(vec![ResourcePattern::path_prefix(PathBuf::from("/tmp"))]),
            );
        assert_eq!(
            policy.reach(&capability("filesystem.read"), RiskLevel::Low),
            Reach::Denied
        );
    }

    #[test]
    fn a_ceiling_below_the_tools_risk_denies_it() {
        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new("run", "terminal", "execute", Effect::Allow))
            .with_max_risk(RiskLevel::Medium);
        assert_eq!(
            policy.reach(&capability("terminal.execute"), RiskLevel::High),
            Reach::Denied
        );

        let policy = Policy::deny_all("p").with_rule(
            PolicyRule::new("run", "terminal", "execute", Effect::Allow)
                .with_max_risk(RiskLevel::Low),
        );
        assert_eq!(
            policy.reach(&capability("terminal.execute"), RiskLevel::High),
            Reach::Denied
        );
        assert_eq!(
            policy.reach(&capability("terminal.execute"), RiskLevel::Low),
            Reach::Allowed
        );
    }

    #[test]
    fn self_modification_is_denied_whatever_the_rules_say() {
        let policy =
            Policy::deny_all("p").with_rule(PolicyRule::new("everything", "*", "*", Effect::Allow));
        assert_eq!(
            policy.reach(&capability("runtime.modify_policy"), RiskLevel::Low),
            Reach::Denied
        );
        assert_eq!(
            policy.reach(&capability("browser.vision"), RiskLevel::Medium),
            Reach::Allowed
        );
    }

    #[test]
    fn a_resource_on_the_capability_asked_about_is_ignored() {
        // The question is about every resource; asking it about one would
        // report a scoped grant as everywhere or nowhere.
        let policy = compiled("default: deny\npermissions:\n  filesystem:\n    read: [\"/tmp\"]\n");
        let scoped = Capability::new("filesystem", "read").with_resource(
            agentos_core::permission::ResourceRef::Path {
                path: "/tmp/a".into(),
            },
        );
        assert_eq!(policy.reach(&scoped, RiskLevel::Low), Reach::Scoped);
    }

    #[test]
    fn where_reach_overstates_it_overstates_and_never_understates() {
        // The shapes the documentation names, each checked against the engine
        // with a call the reach cannot see: one planned above the baseline,
        // one carrying a resource of the wrong kind, one carrying none. Where
        // the engine allows, the reach must say at least Scoped; where it
        // asks, never Denied.
        use agentos_core::permission::{PermissionRequest, ResourceRef};

        use crate::engine::{PermissionEngine, PolicyEngine};

        let floor = |reach: Reach, effect: Effect| match effect {
            Effect::Allow => assert!(reach <= Reach::Scoped, "{reach:?} for an allowed call"),
            Effect::Ask => assert!(reach != Reach::Denied, "denied for an asked call"),
            Effect::Deny => {}
        };
        let path = |capability: Capability| {
            capability.with_resource(ResourceRef::Path {
                path: "/tmp/a".into(),
            })
        };

        // A ceiling at the baseline: the reach says Allowed, an overwrite is
        // refused. The over-report is the documented one.
        let policy = compiled(
            "default: deny\npermissions:\n  filesystem:\n    write: {effect: allow, max_risk: medium}\n",
        );
        let engine = PolicyEngine::new(policy.clone());
        let reach = policy.reach(&capability("filesystem.write"), RiskLevel::Medium);
        assert_eq!(reach, Reach::Allowed);
        for risk in [RiskLevel::Medium, RiskLevel::High] {
            let decision = engine.evaluate(&PermissionRequest::new(
                "filesystem.write",
                path(capability("filesystem.write")),
                risk,
            ));
            floor(reach, decision.effect);
        }

        // A rule naming origins for a capability that carries paths.
        let policy = compiled(
            "default: deny\npermissions:\n  filesystem:\n    read: {origins: ['https://example.com']}\n",
        );
        let engine = PolicyEngine::new(policy.clone());
        let reach = policy.reach(&capability("filesystem.read"), RiskLevel::Low);
        assert_eq!(reach, Reach::Scoped);
        let decision = engine.evaluate(&PermissionRequest::new(
            "filesystem.read",
            path(capability("filesystem.read")),
            RiskLevel::Low,
        ));
        assert_eq!(decision.effect, Effect::Deny);

        // And the property the report exists for, over every shape here and
        // the ordinary ones: nothing the engine grants is reported as denied.
        let policy = compiled(
            "default: deny\nmax_risk: high\npermissions:\n  filesystem:\n    read: ['/tmp']\n    write: {effect: allow, max_risk: medium}\n  email:\n    send: ask\n  computer:\n    click: allow\n",
        );
        let engine = PolicyEngine::new(policy.clone());
        for (name, baseline) in [
            ("filesystem.read", RiskLevel::Low),
            ("filesystem.write", RiskLevel::Medium),
            ("email.send", RiskLevel::High),
            ("computer.click", RiskLevel::Medium),
        ] {
            let reach = policy.reach(&capability(name), baseline);
            for risk in [
                RiskLevel::Low,
                RiskLevel::Medium,
                RiskLevel::High,
                RiskLevel::Critical,
            ] {
                if risk < baseline {
                    continue;
                }
                for request in [capability(name), path(capability(name))] {
                    let decision = engine.evaluate(&PermissionRequest::new(name, request, risk));
                    floor(reach, decision.effect);
                }
            }
        }
    }

    #[test]
    fn the_engine_decides_what_a_uniform_reach_says() {
        // Where the reach is the same for every resource, it must be what the
        // engine decides for a request: the report and the enforcement point
        // read one policy, and may not tell two stories about it.
        use agentos_core::permission::PermissionRequest;

        use crate::engine::{PermissionEngine, PolicyEngine};

        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new("read", "filesystem", "read", Effect::Allow))
            .with_rule(PolicyRule::new("send", "email", "send", Effect::Ask))
            .with_rule(PolicyRule::new("pay", "payments", "execute", Effect::Deny))
            .with_rule(PolicyRule::new("browse", "browser", "*", Effect::Allow))
            .with_rule(
                PolicyRule::new("run", "terminal", "execute", Effect::Allow)
                    .with_max_risk(RiskLevel::Medium),
            );
        let engine = PolicyEngine::new(policy.clone());

        let mut uniform = 0;
        for (name, risk) in [
            ("filesystem.read", RiskLevel::Low),
            ("email.send", RiskLevel::High),
            ("payments.execute", RiskLevel::Critical),
            ("browser.vision", RiskLevel::Medium),
            ("terminal.execute", RiskLevel::High),
            ("calendar.write", RiskLevel::Medium),
        ] {
            let expected = match policy.reach(&capability(name), risk) {
                Reach::Allowed => Effect::Allow,
                Reach::Asks => Effect::Ask,
                Reach::Denied => Effect::Deny,
                Reach::Scoped => continue,
            };
            uniform += 1;
            let decision = engine.evaluate(&PermissionRequest::new(name, capability(name), risk));
            assert_eq!(decision.effect, expected, "{name}");
        }
        assert_eq!(uniform, 6);
    }
}
