//! Permission vocabulary.
//!
//! The *engine* that evaluates these lives in `agentos-permissions`. Only the
//! shapes live here, so that tools can declare what they need without depending
//! on the engine, and so that persistence and audit can record decisions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::risk::RiskLevel;

/// Canonical permission domains shipped with the runtime.
///
/// Plugins may introduce their own; these are the ones core tools use.
pub mod permission_domains {
    /// Local filesystem access.
    pub const FILESYSTEM: &str = "filesystem";
    /// Subprocess execution.
    pub const TERMINAL: &str = "terminal";
    /// Browser automation.
    pub const BROWSER: &str = "browser";
    /// Screen, mouse and keyboard control.
    pub const COMPUTER: &str = "computer";
    /// Network requests not made through the browser.
    pub const NETWORK: &str = "network";
    /// Agent and policy self-modification.
    pub const RUNTIME: &str = "runtime";
}

/// The specific thing an action touches.
///
/// Kept as a small closed enum rather than a free-form string so that the policy
/// engine's matching rules are exhaustive and testable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceRef {
    /// A filesystem path. Always absolute and canonical by the time the engine
    /// sees it — resolution happens before evaluation, never after.
    Path {
        /// The absolute, canonical path.
        path: String,
    },
    /// A program to execute.
    Program {
        /// The program name or absolute path.
        program: String,
    },
    /// A network origin, `scheme://host[:port]`.
    Origin {
        /// The origin.
        origin: String,
    },
    /// An application on the user's desktop, by the name it reports.
    ///
    /// Unlike the other scoped kinds, this value is not resolved by the runtime:
    /// a path is canonicalised and an origin comes from a browser AgentOS itself
    /// launched, but an application name is whatever the process in front says
    /// it is called. It scopes input to the window the operator meant; it is not
    /// proof of identity.
    Application {
        /// The application name, e.g. `Mail`.
        application: String,
    },
    /// A named non-path resource, e.g. an integration account.
    Named {
        /// The resource name.
        name: String,
    },
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path { path } => write!(f, "path:{path}"),
            Self::Program { program } => write!(f, "program:{program}"),
            Self::Origin { origin } => write!(f, "origin:{origin}"),
            Self::Application { application } => write!(f, "application:{application}"),
            Self::Named { name } => write!(f, "name:{name}"),
        }
    }
}

/// A capability an action requires: "write to this path", "execute this program".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    /// Domain, e.g. `filesystem`. See [`permission_domains`].
    pub domain: String,
    /// Action within the domain, e.g. `write`.
    pub action: String,
    /// The resource touched, when the capability is resource-scoped.
    ///
    /// `None` means the capability is unscoped — the policy may still restrict
    /// it, but there is no resource to match patterns against.
    pub resource: Option<ResourceRef>,
}

impl Capability {
    /// An unscoped capability.
    #[must_use]
    pub fn new(domain: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            action: action.into(),
            resource: None,
        }
    }

    /// Attach the resource this capability applies to.
    #[must_use]
    pub fn with_resource(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    /// `domain.action`, the form used in policy files and audit rows.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.domain, self.action)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.resource {
            Some(resource) => write!(f, "{}.{} on {resource}", self.domain, self.action),
            None => write!(f, "{}.{}", self.domain, self.action),
        }
    }
}

/// What a policy says about a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Proceed without asking.
    Allow,
    /// Proceed only after a human approves.
    Ask,
    /// Refuse.
    Deny,
}

impl Effect {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    /// Precedence weight. Higher wins when rules of equal specificity conflict.
    ///
    /// `Deny` beating `Ask` beating `Allow` means a conflicting policy always
    /// fails closed.
    #[must_use]
    pub const fn precedence(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Ask => 1,
            Self::Deny => 2,
        }
    }

    /// The more restrictive of two effects.
    #[must_use]
    pub fn stricter(self, other: Self) -> Self {
        if self.precedence() >= other.precedence() {
            self
        } else {
            other
        }
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A question put to the policy engine.
///
/// Every field is supplied by the runtime from validated data. The model has no
/// influence over anything here except `capability.resource`, which the runtime
/// derives from already-validated tool arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// The tool being invoked.
    pub tool: String,
    /// The capability it needs.
    pub capability: Capability,
    /// The risk of this specific invocation, after per-argument adjustment.
    pub risk: RiskLevel,
    /// Whether the run has already ingested untrusted data.
    ///
    /// This is the taint signal: a run that has read a webpage is treated more
    /// conservatively than one that has not.
    pub tainted: bool,
}

impl PermissionRequest {
    /// Build a request.
    #[must_use]
    pub fn new(tool: impl Into<String>, capability: Capability, risk: RiskLevel) -> Self {
        Self {
            tool: tool.into(),
            capability,
            risk,
            tainted: false,
        }
    }

    /// Mark the run as having ingested untrusted data.
    #[must_use]
    pub const fn tainted(mut self, tainted: bool) -> Self {
        self.tainted = tainted;
        self
    }
}

/// The engine's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecision {
    /// What to do.
    pub effect: Effect,
    /// Human-readable justification, shown in approval prompts and audit rows.
    pub reason: String,
    /// The policy rule that decided it, if any. `None` means the default applied.
    pub matched_rule: Option<String>,
    /// The effect the rules produced before taint escalation was applied.
    ///
    /// Recorded so the audit log can distinguish "policy said ask" from
    /// "policy said allow, but the run was tainted".
    pub effect_before_taint: Effect,
}

impl PermissionDecision {
    /// Build a decision.
    #[must_use]
    pub fn new(effect: Effect, reason: impl Into<String>) -> Self {
        Self {
            effect,
            reason: reason.into(),
            matched_rule: None,
            effect_before_taint: effect,
        }
    }

    /// Record which rule matched.
    #[must_use]
    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.matched_rule = Some(rule.into());
        self
    }

    /// Record the pre-escalation effect.
    #[must_use]
    pub const fn with_effect_before_taint(mut self, effect: Effect) -> Self {
        self.effect_before_taint = effect;
        self
    }

    /// Whether taint escalation changed the outcome.
    #[must_use]
    pub fn was_escalated_by_taint(&self) -> bool {
        self.effect != self.effect_before_taint
    }

    /// Whether the action may proceed, possibly after approval.
    #[must_use]
    pub const fn is_permitted(&self) -> bool {
        matches!(self.effect, Effect::Allow | Effect::Ask)
    }

    /// Whether a human must approve first.
    #[must_use]
    pub const fn requires_approval(&self) -> bool {
        matches!(self.effect, Effect::Ask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_beats_ask_beats_allow() {
        assert_eq!(Effect::Allow.stricter(Effect::Ask), Effect::Ask);
        assert_eq!(Effect::Ask.stricter(Effect::Deny), Effect::Deny);
        assert_eq!(Effect::Deny.stricter(Effect::Allow), Effect::Deny);
        assert_eq!(Effect::Allow.stricter(Effect::Allow), Effect::Allow);
    }

    #[test]
    fn qualified_names_round_trip_into_display() {
        let cap = Capability::new("filesystem", "write").with_resource(ResourceRef::Path {
            path: "/tmp/x".into(),
        });
        assert_eq!(cap.qualified_name(), "filesystem.write");
        assert_eq!(cap.to_string(), "filesystem.write on path:/tmp/x");
    }

    #[test]
    fn every_resource_kind_renders_with_its_own_prefix() {
        // Rules are compared by their rendered form, so two kinds sharing a
        // prefix would be indistinguishable — an application rule and an
        // integration-account rule of the same name would collapse into one.
        let rendered = [
            ResourceRef::Path {
                path: "/tmp/x".into(),
            },
            ResourceRef::Program {
                program: "git".into(),
            },
            ResourceRef::Origin {
                origin: "https://example.com".into(),
            },
            ResourceRef::Application {
                application: "Mail".into(),
            },
            ResourceRef::Named {
                name: "Mail".into(),
            },
        ]
        .map(|resource| resource.to_string());

        let prefixes: Vec<&str> = rendered
            .iter()
            .filter_map(|text| text.split_once(':'))
            .map(|(prefix, _)| prefix)
            .collect();
        assert_eq!(
            prefixes,
            vec!["path", "program", "origin", "application", "name"]
        );
        assert_eq!(rendered[3], "application:Mail");
        assert_ne!(rendered[3], rendered[4]);
    }

    #[test]
    fn denied_decisions_are_not_permitted() {
        let decision = PermissionDecision::new(Effect::Deny, "no");
        assert!(!decision.is_permitted());
        assert!(!decision.requires_approval());
    }

    #[test]
    fn taint_escalation_is_detectable() {
        let decision = PermissionDecision::new(Effect::Ask, "tainted run")
            .with_effect_before_taint(Effect::Allow);
        assert!(decision.was_escalated_by_taint());
        assert!(decision.requires_approval());
    }
}
