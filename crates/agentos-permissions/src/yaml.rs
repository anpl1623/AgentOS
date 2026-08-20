//! Loading policies from YAML.
//!
//! The document shape follows the one in the project specification, with a
//! shorthand for the common cases:
//!
//! ```yaml
//! agent: sales
//! default: deny
//! max_risk: high
//!
//! taint_escalation:
//!   enabled: true
//!   escalate_at_or_above: medium
//!
//! permissions:
//!   computer:
//!     mouse: allow
//!     keyboard: allow
//!
//!   filesystem:
//!     read: [~/Documents/Sales]        # shorthand: allow, scoped to these paths
//!     write:
//!       effect: allow
//!       paths: [~/Documents/Sales]
//!     delete:
//!       effect: ask
//!       paths: [~/Documents/Sales]
//!
//!   terminal:
//!     exec:
//!       effect: ask
//!       programs: [git, npm, cargo]
//!
//!   browser:
//!     navigate:
//!       effect: allow
//!       origins: ["https://*.example.com", "http://localhost:*"]
//!
//!   email:
//!     send: ask
//!
//!   payments:
//!     execute: deny
//! ```
//!
//! Paths are expanded (`~`) and canonicalised at load time, so the engine only
//! ever compares canonical paths. A path that does not exist yet is resolved
//! against its nearest existing ancestor rather than rejected — an agent scoped
//! to a directory it will create on first use is a normal configuration.

use std::collections::BTreeMap;
use std::path::Path;

use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use serde::{Deserialize, Serialize};

use crate::error::PolicyError;
use crate::path::{expand_home, resolve_secure};
use crate::pattern::{GlobKind, ResourcePattern};
use crate::policy::{Policy, PolicyRule, TaintPolicy};

/// The YAML document, before it is compiled into a [`Policy`].
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    /// Agent name this policy belongs to.
    #[serde(default)]
    pub agent: Option<String>,
    /// Effect when no rule matches. Defaults to `deny`.
    #[serde(default = "default_effect")]
    pub default: Effect,
    /// Global risk ceiling.
    #[serde(default)]
    pub max_risk: Option<RiskLevel>,
    /// Taint escalation settings.
    #[serde(default)]
    pub taint_escalation: Option<TaintDocument>,
    /// Domain -> action -> specification.
    ///
    /// `BTreeMap` rather than `HashMap` so compilation is deterministic and rule
    /// identifiers are stable between loads.
    #[serde(default)]
    pub permissions: BTreeMap<String, BTreeMap<String, ActionSpec>>,
}

const fn default_effect() -> Effect {
    Effect::Deny
}

/// Taint escalation settings as written in YAML.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaintDocument {
    /// Whether escalation applies.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Risk level at which escalation kicks in.
    #[serde(default = "default_taint_threshold")]
    pub escalate_at_or_above: RiskLevel,
}

const fn default_true() -> bool {
    true
}

const fn default_taint_threshold() -> RiskLevel {
    RiskLevel::Medium
}

impl From<TaintDocument> for TaintPolicy {
    fn from(doc: TaintDocument) -> Self {
        Self {
            enabled: doc.enabled,
            escalate_at_or_above: doc.escalate_at_or_above,
        }
    }
}

/// What a policy says about one action.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionSpec {
    /// `send: ask`
    Effect(Effect),
    /// `read: [~/Docs]` — shorthand for allow, scoped to those resources. The
    /// resource kind is inferred from the domain.
    Resources(Vec<String>),
    /// The explicit form.
    Detailed(DetailedSpec),
}

/// The explicit long form of an action specification.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DetailedSpec {
    /// What to do. Defaults to `allow` when resources are listed.
    #[serde(default)]
    pub effect: Option<Effect>,
    /// Filesystem roots.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Program names or globs.
    #[serde(default)]
    pub programs: Vec<String>,
    /// Network origins or globs.
    #[serde(default)]
    pub origins: Vec<String>,
    /// Named resources or globs.
    #[serde(default)]
    pub names: Vec<String>,
    /// Risk ceiling for this action.
    #[serde(default)]
    pub max_risk: Option<RiskLevel>,
}

impl PolicyDocument {
    /// Parse a YAML document.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Yaml`] if the document is malformed.
    pub fn from_yaml(source: &str) -> Result<Self, PolicyError> {
        serde_yaml_ng::from_str(source).map_err(PolicyError::Yaml)
    }

    /// Compile into an executable [`Policy`].
    ///
    /// Path roots are expanded and canonicalised here so that the engine never
    /// has to touch the filesystem during evaluation.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] if a glob is malformed, a path cannot be
    /// resolved, or `~` cannot be expanded.
    pub fn compile(&self) -> Result<Policy, PolicyError> {
        let name = self.agent.clone().unwrap_or_else(|| "policy".to_owned());
        let mut policy = Policy {
            name,
            default_effect: self.default,
            max_risk: self.max_risk,
            rules: Vec::new(),
            taint: self.taint_escalation.map(Into::into).unwrap_or_default(),
        };

        for (domain, actions) in &self.permissions {
            for (action, spec) in actions {
                let rule_id = format!("{domain}.{action}");
                policy
                    .rules
                    .push(compile_rule(&rule_id, domain, action, spec)?);
            }
        }

        Ok(policy)
    }
}

fn compile_rule(
    rule_id: &str,
    domain: &str,
    action: &str,
    spec: &ActionSpec,
) -> Result<PolicyRule, PolicyError> {
    let detailed = match spec {
        ActionSpec::Effect(effect) => DetailedSpec {
            effect: Some(*effect),
            ..DetailedSpec::default()
        },
        ActionSpec::Resources(resources) => shorthand_to_detailed(domain, resources),
        ActionSpec::Detailed(detailed) => detailed.clone(),
    };

    let mut patterns = Vec::new();
    for raw in &detailed.paths {
        patterns.push(compile_path(rule_id, raw)?);
    }
    for (kind, values) in [
        (GlobKind::Program, &detailed.programs),
        (GlobKind::Origin, &detailed.origins),
        (GlobKind::Named, &detailed.names),
    ] {
        for raw in values {
            patterns.push(ResourcePattern::glob(kind, raw).map_err(|source| {
                PolicyError::Pattern {
                    pattern: raw.clone(),
                    rule: rule_id.to_owned(),
                    source,
                }
            })?);
        }
    }

    // Listing resources without an effect means "allow, but only here". Listing
    // neither means the effect must be explicit.
    let effect = match (detailed.effect, patterns.is_empty()) {
        (Some(effect), _) => effect,
        (None, false) => Effect::Allow,
        (None, true) => {
            return Err(PolicyError::Invalid(format!(
                "`{rule_id}` specifies neither an effect nor any resources"
            )));
        }
    };

    let mut rule = PolicyRule::new(rule_id, domain, action, effect).with_resources(patterns);
    if let Some(max_risk) = detailed.max_risk {
        rule = rule.with_max_risk(max_risk);
    }
    Ok(rule)
}

/// Infer the resource kind for the `action: [values]` shorthand from the domain.
fn shorthand_to_detailed(domain: &str, resources: &[String]) -> DetailedSpec {
    use agentos_core::permission::permission_domains as domains;

    let values = resources.to_vec();
    match domain {
        domains::FILESYSTEM => DetailedSpec {
            paths: values,
            ..DetailedSpec::default()
        },
        domains::TERMINAL => DetailedSpec {
            programs: values,
            ..DetailedSpec::default()
        },
        domains::BROWSER | domains::NETWORK => DetailedSpec {
            origins: values,
            ..DetailedSpec::default()
        },
        _ => DetailedSpec {
            names: values,
            ..DetailedSpec::default()
        },
    }
}

fn compile_path(rule_id: &str, raw: &str) -> Result<ResourcePattern, PolicyError> {
    let expanded = expand_home(raw).ok_or_else(|| PolicyError::NoHomeDirectory {
        path: raw.to_owned(),
    })?;

    if !expanded.is_absolute() {
        return Err(PolicyError::Invalid(format!(
            "filesystem root `{raw}` in rule `{rule_id}` must be absolute"
        )));
    }

    let canonical = resolve_secure(&expanded).map_err(|error| PolicyError::Root {
        path: expanded.clone(),
        rule: rule_id.to_owned(),
        source: to_io_error(&error),
    })?;

    Ok(ResourcePattern::path_prefix(canonical))
}

fn to_io_error(error: &crate::error::PathError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

/// Load and compile a policy from a file.
///
/// # Errors
///
/// Returns [`PolicyError`] for I/O failures, malformed YAML, or bad patterns.
pub fn load_policy_file(path: &Path) -> Result<Policy, PolicyError> {
    let source = std::fs::read_to_string(path).map_err(|source| PolicyError::Root {
        path: path.to_path_buf(),
        rule: "<file>".to_owned(),
        source,
    })?;
    PolicyDocument::from_yaml(&source)?.compile()
}

/// The starter policy shipped with a freshly created agent.
///
/// Read-only inside one workspace directory, browsing allowed on localhost,
/// everything else denied. Deliberately close to useless until an operator
/// widens it — the default must never be the permissive one.
#[must_use]
pub fn starter_policy_yaml(workspace: &Path) -> String {
    let workspace = workspace.display();
    format!(
        "# AgentOS starter policy — deny by default.\n\
         # Widen deliberately; every line here is a capability you are granting.\n\
         default: deny\n\
         max_risk: medium\n\
         \n\
         taint_escalation:\n\
         \x20 enabled: true\n\
         \x20 escalate_at_or_above: medium\n\
         \n\
         permissions:\n\
         \x20 filesystem:\n\
         \x20   read: [\"{workspace}\"]\n\
         \x20   list: [\"{workspace}\"]\n\
         \x20   write:\n\
         \x20     effect: ask\n\
         \x20     paths: [\"{workspace}\"]\n\
         \x20 browser:\n\
         \x20   navigate:\n\
         \x20     effect: allow\n\
         \x20     origins: [\"http://localhost:*\", \"http://127.0.0.1:*\"]\n\
         \x20   interact:\n\
         \x20     effect: allow\n\
         \x20     origins: [\"http://localhost:*\", \"http://127.0.0.1:*\"]\n\
         \x20   read:\n\
         \x20     effect: allow\n\
         \x20     origins: [\"http://localhost:*\", \"http://127.0.0.1:*\"]\n"
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use agentos_core::permission::{Capability, ResourceRef};
    use tempfile::TempDir;

    use super::*;
    use crate::engine::{PermissionEngine, PolicyEngine};
    use agentos_core::permission::PermissionRequest;

    fn canonical_temp() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        (dir, root)
    }

    #[test]
    fn parses_the_specification_example() {
        let (_guard, root) = canonical_temp();
        let yaml = format!(
            r#"
agent: sales
default: deny
permissions:
  computer:
    mouse: allow
    keyboard: allow
    screenshot: allow
  browser:
    navigation: allow
    interaction: allow
  filesystem:
    read:
      - {root}
    write:
      - {root}
  email:
    read: allow
    draft: allow
    send: ask
  payments:
    execute: deny
"#,
            root = root.display()
        );

        let policy = PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap();
        assert_eq!(policy.name, "sales");
        assert_eq!(policy.default_effect, Effect::Deny);
        assert_eq!(policy.rules.len(), 11);

        let engine = PolicyEngine::new(policy);
        let send = PermissionRequest::new(
            "email.send",
            Capability::new("email", "send"),
            RiskLevel::High,
        );
        assert_eq!(engine.evaluate(&send).effect, Effect::Ask);

        let pay = PermissionRequest::new(
            "payments.execute",
            Capability::new("payments", "execute"),
            RiskLevel::Critical,
        );
        assert_eq!(engine.evaluate(&pay).effect, Effect::Deny);
    }

    #[test]
    fn shorthand_paths_scope_the_rule() {
        let (_guard, root) = canonical_temp();
        let yaml = format!(
            "permissions:\n  filesystem:\n    read:\n      - {}\n",
            root.display()
        );
        let engine =
            PolicyEngine::new(PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap());

        let inside = PermissionRequest::new(
            "filesystem.read",
            Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
                path: root.join("a.txt").display().to_string(),
            }),
            RiskLevel::Low,
        );
        assert_eq!(engine.evaluate(&inside).effect, Effect::Allow);

        let outside = PermissionRequest::new(
            "filesystem.read",
            Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
                path: "/etc/passwd".into(),
            }),
            RiskLevel::Low,
        );
        assert_eq!(engine.evaluate(&outside).effect, Effect::Deny);
    }

    #[test]
    fn terminal_shorthand_infers_program_patterns() {
        let yaml = "permissions:\n  terminal:\n    exec: [git, cargo]\n";
        let engine = PolicyEngine::new(PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap());

        let allowed = PermissionRequest::new(
            "terminal.exec",
            Capability::new("terminal", "exec").with_resource(ResourceRef::Program {
                program: "git".into(),
            }),
            RiskLevel::Medium,
        );
        assert_eq!(engine.evaluate(&allowed).effect, Effect::Allow);

        let denied = PermissionRequest::new(
            "terminal.exec",
            Capability::new("terminal", "exec").with_resource(ResourceRef::Program {
                program: "curl".into(),
            }),
            RiskLevel::Medium,
        );
        assert_eq!(engine.evaluate(&denied).effect, Effect::Deny);
    }

    #[test]
    fn browser_shorthand_infers_origin_patterns() {
        let yaml = "permissions:\n  browser:\n    navigate: [\"http://localhost:*\"]\n";
        let engine = PolicyEngine::new(PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap());

        let local = PermissionRequest::new(
            "browser.navigate",
            Capability::new("browser", "navigate").with_resource(ResourceRef::Origin {
                origin: "http://localhost:8420".into(),
            }),
            RiskLevel::Medium,
        );
        assert_eq!(engine.evaluate(&local).effect, Effect::Allow);

        let remote = PermissionRequest::new(
            "browser.navigate",
            Capability::new("browser", "navigate").with_resource(ResourceRef::Origin {
                origin: "https://evil.example".into(),
            }),
            RiskLevel::Medium,
        );
        assert_eq!(engine.evaluate(&remote).effect, Effect::Deny);
    }

    #[test]
    fn detailed_form_supports_ask_with_scope_and_ceiling() {
        let (_guard, root) = canonical_temp();
        let yaml = format!(
            "permissions:\n  filesystem:\n    write:\n      effect: ask\n      paths: [{}]\n      max_risk: medium\n",
            root.display()
        );
        let policy = PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap();
        let rule = &policy.rules[0];
        assert_eq!(rule.effect, Effect::Ask);
        assert_eq!(rule.max_risk, Some(RiskLevel::Medium));
    }

    #[test]
    fn taint_settings_are_read() {
        let yaml =
            "taint_escalation:\n  enabled: false\n  escalate_at_or_above: high\npermissions: {}\n";
        let policy = PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap();
        assert!(!policy.taint.enabled);
        assert_eq!(policy.taint.escalate_at_or_above, RiskLevel::High);
    }

    #[test]
    fn default_is_deny_when_unspecified() {
        let policy = PolicyDocument::from_yaml("permissions: {}\n")
            .unwrap()
            .compile()
            .unwrap();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert!(policy.taint.enabled);
    }

    #[test]
    fn unknown_top_level_fields_are_rejected() {
        // Silently ignoring a misspelled `permisions:` key would produce a
        // policy that grants nothing while looking like it grants plenty.
        let err = PolicyDocument::from_yaml("permisions: {}\n").unwrap_err();
        assert!(matches!(err, PolicyError::Yaml(_)));
    }

    #[test]
    fn relative_paths_are_rejected() {
        let yaml = "permissions:\n  filesystem:\n    read: [\"relative/dir\"]\n";
        let err = PolicyDocument::from_yaml(yaml)
            .unwrap()
            .compile()
            .unwrap_err();
        assert!(matches!(err, PolicyError::Invalid(_)));
    }

    #[test]
    fn an_action_with_neither_effect_nor_resources_is_rejected() {
        let yaml = "permissions:\n  filesystem:\n    read: {}\n";
        let err = PolicyDocument::from_yaml(yaml)
            .unwrap()
            .compile()
            .unwrap_err();
        assert!(matches!(err, PolicyError::Invalid(_)));
    }

    #[test]
    fn paths_that_do_not_exist_yet_are_allowed_as_roots() {
        let (_guard, root) = canonical_temp();
        let future = root.join("will-exist-later");
        let yaml = format!(
            "permissions:\n  filesystem:\n    write: [\"{}\"]\n",
            future.display()
        );
        let policy = PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap();
        assert_eq!(policy.rules.len(), 1);
    }

    #[test]
    fn starter_policy_compiles_and_denies_by_default() {
        let (_guard, root) = canonical_temp();
        let yaml = starter_policy_yaml(&root);
        let policy = PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert_eq!(policy.max_risk, Some(RiskLevel::Medium));

        let engine = PolicyEngine::new(policy);
        let exec = PermissionRequest::new(
            "terminal.exec",
            Capability::new("terminal", "exec").with_resource(ResourceRef::Program {
                program: "rm".into(),
            }),
            RiskLevel::High,
        );
        assert_eq!(engine.evaluate(&exec).effect, Effect::Deny);
    }

    #[test]
    fn rule_ids_are_stable_across_loads() {
        let yaml = "permissions:\n  email:\n    send: ask\n    draft: allow\n";
        let a = PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap();
        let b = PolicyDocument::from_yaml(yaml).unwrap().compile().unwrap();
        let ids = |p: &Policy| p.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        assert_eq!(ids(&a), vec!["email.draft", "email.send"]);
    }
}
