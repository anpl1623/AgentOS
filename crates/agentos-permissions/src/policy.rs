//! Policy documents and the rules inside them.

use agentos_core::permission::{Capability, Effect};
use agentos_core::risk::RiskLevel;

use crate::pattern::{NamePattern, ResourcePattern};

/// One rule: "for capabilities matching this shape, do this".
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyRule {
    /// Stable identifier, recorded on every decision this rule produces so an
    /// audit reader can trace a denial back to the line that caused it.
    pub id: String,
    /// Which domain this applies to.
    pub domain: NamePattern,
    /// Which action this applies to.
    pub action: NamePattern,
    /// Which resources this applies to. Empty means [`ResourcePattern::Any`].
    pub resources: Vec<ResourcePattern>,
    /// What to do when it matches.
    pub effect: Effect,
    /// Risk ceiling for this rule. Actions above it are denied even if the
    /// effect is `allow`.
    pub max_risk: Option<RiskLevel>,
}

impl PolicyRule {
    /// A rule matching a whole domain and action, with no resource scoping.
    #[must_use]
    pub fn new(id: impl Into<String>, domain: &str, action: &str, effect: Effect) -> Self {
        Self {
            id: id.into(),
            domain: NamePattern::parse(domain),
            action: NamePattern::parse(action),
            resources: Vec::new(),
            effect,
            max_risk: None,
        }
    }

    /// Scope the rule to specific resources.
    #[must_use]
    pub fn with_resources(mut self, resources: Vec<ResourcePattern>) -> Self {
        self.resources = resources;
        self
    }

    /// Cap the risk this rule will permit.
    #[must_use]
    pub const fn with_max_risk(mut self, max_risk: RiskLevel) -> Self {
        self.max_risk = Some(max_risk);
        self
    }

    /// Whether this rule applies to `capability`.
    #[must_use]
    pub fn matches(&self, capability: &Capability) -> bool {
        if !self.domain.matches(&capability.domain) || !self.action.matches(&capability.action) {
            return false;
        }
        if self.resources.is_empty() {
            return true;
        }
        self.resources
            .iter()
            .any(|pattern| pattern.matches(capability.resource.as_ref()))
    }

    /// How specific this rule is, as a comparable tuple.
    ///
    /// Compared lexicographically: domain, then action, then resource. This
    /// makes "most specific wins" deterministic instead of order-dependent.
    #[must_use]
    pub fn specificity(&self, capability: &Capability) -> (u32, u32, u32) {
        let resource = self
            .resources
            .iter()
            .filter(|pattern| pattern.matches(capability.resource.as_ref()))
            .map(ResourcePattern::specificity)
            .max()
            .unwrap_or(0);
        (
            self.domain.specificity(),
            self.action.specificity(),
            resource,
        )
    }

    /// Human-readable description for audit messages.
    #[must_use]
    pub fn describe(&self) -> String {
        let resources = if self.resources.is_empty() {
            String::new()
        } else {
            format!(
                " on [{}]",
                self.resources
                    .iter()
                    .map(ResourcePattern::describe)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!("{} => {}{resources}", self.id, self.effect)
    }
}

/// How aggressively to escalate once a run has read untrusted data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaintPolicy {
    /// Whether escalation applies at all.
    pub enabled: bool,
    /// Once tainted, actions at or above this risk require approval even if the
    /// policy would otherwise allow them silently.
    pub escalate_at_or_above: RiskLevel,
}

impl Default for TaintPolicy {
    fn default() -> Self {
        // Medium is the level at which an action first has an effect the
        // operator would want to know about: writing a file, or reaching the
        // network. Reading in-scope local state stays silent.
        Self {
            enabled: true,
            escalate_at_or_above: RiskLevel::Medium,
        }
    }
}

/// Capabilities no policy may ever grant.
///
/// These are the self-modification paths. If an agent could edit its own policy
/// or rewrite its own instructions, every other control here would be advisory.
/// The check runs before rule evaluation, so no document can override it — not
/// even one the operator wrote by mistake.
pub const IMMUTABLE_DENY: &[(&str, &str)] = &[
    ("runtime", "modify_policy"),
    ("runtime", "modify_agent"),
    ("runtime", "disable_audit"),
    ("runtime", "disable_approvals"),
];

/// Whether a capability is permanently denied.
#[must_use]
pub fn is_immutably_denied(capability: &Capability) -> bool {
    IMMUTABLE_DENY
        .iter()
        .any(|(domain, action)| capability.domain == *domain && capability.action == *action)
}

/// A complete permission policy for one agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Human-readable name, usually the agent's.
    pub name: String,
    /// What to do when no rule matches. Deny, unless deliberately changed.
    pub default_effect: Effect,
    /// Global risk ceiling. Actions above it are denied regardless of rules.
    pub max_risk: Option<RiskLevel>,
    /// The rules, in no particular order — specificity decides, not position.
    pub rules: Vec<PolicyRule>,
    /// Taint escalation settings.
    pub taint: TaintPolicy,
}

impl Default for Policy {
    fn default() -> Self {
        Self::deny_all("default")
    }
}

impl Policy {
    /// A policy that permits nothing. The correct starting point for a new agent.
    #[must_use]
    pub fn deny_all(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            default_effect: Effect::Deny,
            max_risk: None,
            rules: Vec::new(),
            taint: TaintPolicy::default(),
        }
    }

    /// Add a rule.
    #[must_use]
    pub fn with_rule(mut self, rule: PolicyRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set the global risk ceiling.
    #[must_use]
    pub const fn with_max_risk(mut self, max_risk: RiskLevel) -> Self {
        self.max_risk = Some(max_risk);
        self
    }

    /// Replace the taint settings.
    #[must_use]
    pub const fn with_taint_policy(mut self, taint: TaintPolicy) -> Self {
        self.taint = taint;
        self
    }

    /// The rule that governs `capability`, if any.
    ///
    /// The winner is the most specific match; ties go to the stricter effect, so
    /// a contradictory policy fails closed rather than picking whichever rule
    /// happened to be listed first.
    #[must_use]
    pub fn winning_rule(&self, capability: &Capability) -> Option<&PolicyRule> {
        self.rules
            .iter()
            .filter(|rule| rule.matches(capability))
            .max_by(|a, b| {
                a.specificity(capability)
                    .cmp(&b.specificity(capability))
                    .then_with(|| a.effect.precedence().cmp(&b.effect.precedence()))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use agentos_core::permission::ResourceRef;

    use super::*;

    fn write_to(path: &str) -> Capability {
        Capability::new("filesystem", "write")
            .with_resource(ResourceRef::Path { path: path.into() })
    }

    #[test]
    fn default_policy_denies_everything() {
        let policy = Policy::default();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert!(policy.winning_rule(&write_to("/anything")).is_none());
    }

    #[test]
    fn most_specific_rule_wins_regardless_of_order() {
        let broad = PolicyRule::new("broad", "filesystem", "*", Effect::Deny);
        let narrow = PolicyRule::new("narrow", "filesystem", "write", Effect::Allow);

        let a = Policy::deny_all("a")
            .with_rule(broad.clone())
            .with_rule(narrow.clone());
        let b = Policy::deny_all("b").with_rule(narrow).with_rule(broad);

        let cap = Capability::new("filesystem", "write");
        assert_eq!(a.winning_rule(&cap).map(|r| r.id.as_str()), Some("narrow"));
        assert_eq!(b.winning_rule(&cap).map(|r| r.id.as_str()), Some("narrow"));
    }

    #[test]
    fn deeper_path_scope_beats_shallower() {
        let policy = Policy::deny_all("p")
            .with_rule(
                PolicyRule::new("home", "filesystem", "write", Effect::Allow)
                    .with_resources(vec![ResourcePattern::path_prefix(PathBuf::from("/home/u"))]),
            )
            .with_rule(
                PolicyRule::new("secrets", "filesystem", "write", Effect::Deny).with_resources(
                    vec![ResourcePattern::path_prefix(PathBuf::from("/home/u/.ssh"))],
                ),
            );

        assert_eq!(
            policy
                .winning_rule(&write_to("/home/u/notes.txt"))
                .map(|r| r.id.as_str()),
            Some("home")
        );
        assert_eq!(
            policy
                .winning_rule(&write_to("/home/u/.ssh/id_rsa"))
                .map(|r| r.id.as_str()),
            Some("secrets")
        );
    }

    #[test]
    fn equal_specificity_ties_go_to_the_stricter_effect() {
        let policy = Policy::deny_all("p")
            .with_rule(PolicyRule::new("yes", "email", "send", Effect::Allow))
            .with_rule(PolicyRule::new("no", "email", "send", Effect::Deny));
        let winner = policy
            .winning_rule(&Capability::new("email", "send"))
            .map(|r| r.effect);
        assert_eq!(winner, Some(Effect::Deny));
    }

    #[test]
    fn self_modification_is_immutably_denied() {
        for (domain, action) in IMMUTABLE_DENY {
            assert!(is_immutably_denied(&Capability::new(*domain, *action)));
        }
        assert!(!is_immutably_denied(&Capability::new("filesystem", "read")));
    }

    #[test]
    fn taint_defaults_to_enabled_at_medium() {
        let taint = TaintPolicy::default();
        assert!(taint.enabled);
        assert_eq!(taint.escalate_at_or_above, RiskLevel::Medium);
    }
}
