//! The [`Tool`] trait and the registry.

mod context;
mod plan;
mod secret;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use agentos_core::permission::Capability;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

use crate::error::ToolError;

pub use context::{
    DEFAULT_MAX_IMAGES, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT, PolicyProbe, ToolContext,
};
pub use plan::{ToolOutput, ToolPlan};
pub use secret::{CredentialResolver, MIN_REDACTED_FRAGMENT, REDACTED_CREDENTIAL, Secret};

pub(crate) use secret::{CredentialLedger, redact_values};

/// An executable capability.
///
/// Implementors do three separable things, in this order: check the arguments,
/// say what the call would do, then do it. Keeping them separate is what allows
/// the runtime to authorise and to ask a human *before* any side effect occurs.
#[async_trait]
pub trait Tool: Send + Sync + fmt::Debug {
    /// What this tool advertises.
    fn metadata(&self) -> &ToolMetadata;

    /// Check the model's raw arguments against the schema.
    ///
    /// Returns the canonical validated form. Validation is deserialisation into
    /// the tool's typed argument struct, which rejects unknown fields — a model
    /// cannot smuggle an extra parameter past a tool that does not expect one.
    ///
    /// # Errors
    ///
    /// [`ToolError::InvalidArguments`] if the arguments do not fit.
    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError>;

    /// Describe what invoking with these validated arguments would do.
    ///
    /// Async because planning legitimately needs to look at the world — does
    /// this path already exist, what page is the browser on — to say what the
    /// call would actually do. It must remain free of *side effects*: this runs
    /// before authorisation, and the whole model depends on nothing having
    /// happened by the time the policy engine is consulted.
    ///
    /// # Errors
    ///
    /// [`ToolError`] if the arguments cannot be resolved into a concrete plan —
    /// for example a path that cannot be canonicalised.
    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError>;

    /// The longest one call may run before the pipeline abandons it.
    ///
    /// `context.timeout` for any tool that does its own work. `None` only for
    /// a tool whose call is a container for work that is limited on its own
    /// terms: a delegation runs other agents, every model request and tool
    /// call of which already carries its own limit, and which a person may
    /// keep waiting at an approval for as long as a person likes. Cutting the
    /// container off at a fixed length would fail exactly the delegations
    /// that are working. Such a tool is still stopped by `cancel`.
    fn time_limit(&self, context: &ToolContext) -> Option<Duration> {
        Some(context.timeout)
    }

    /// Execute.
    ///
    /// Must honour `cancel` promptly and must not exceed
    /// [`Self::time_limit`]; the pipeline enforces the limit as a backstop,
    /// but a tool that leaves a subprocess running after being cancelled has
    /// leaked it.
    ///
    /// # Errors
    ///
    /// Any [`ToolError`].
    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError>;

    /// Release anything this tool was holding for a run that has finished.
    ///
    /// Tools are shared across every run, so per-run resources — a browser
    /// process, a connection, a temporary directory — cannot live in the tool
    /// itself. They live in a pool keyed by run, and this is how the runtime
    /// says a key is dead. The default does nothing, which is right for the
    /// tools that hold no state.
    async fn end_run(&self, _run_id: agentos_core::ids::TaskRunId) {}
}

/// Deserialise validated arguments into a tool's typed struct.
///
/// # Errors
///
/// [`ToolError::InvalidArguments`] if they do not fit.
pub fn parse_arguments<T: DeserializeOwned>(
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<T, ToolError> {
    serde_json::from_value(arguments.clone())
        .map_err(|error| ToolError::invalid(tool, error.to_string()))
}

/// The set of tools a runtime knows about.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tool, replacing any tool of the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.metadata().name.clone();
        self.tools.insert(name, tool);
    }

    /// Look up a tool.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Every registered tool name, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// How many tools are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Metadata for the tools an agent has enabled, in registry order.
    ///
    /// This is the list advertised to the model. It is a convenience filter, not
    /// a security boundary: a name absent here simply is not offered, while a
    /// name present here is still subject to the policy engine.
    #[must_use]
    pub fn metadata_for(&self, enabled: &[String]) -> Vec<ToolMetadata> {
        self.tools
            .values()
            .filter(|tool| enabled.iter().any(|name| *name == tool.metadata().name))
            .map(|tool| tool.metadata().clone())
            .collect()
    }

    /// Metadata for every registered tool.
    #[must_use]
    pub fn all_metadata(&self) -> Vec<ToolMetadata> {
        self.tools
            .values()
            .map(|tool| tool.metadata().clone())
            .collect()
    }

    /// Tell every tool that a run has finished, so it can release resources.
    pub async fn end_run(&self, run_id: agentos_core::ids::TaskRunId) {
        for tool in self.tools.values() {
            tool.end_run(run_id).await;
        }
    }
}

/// Build a [`ToolMetadata`] from a typed argument struct.
///
/// The schema advertised to the model and the struct used to validate its reply
/// come from the same type, so the two cannot drift apart.
#[must_use]
pub fn metadata_for<T: schemars::JsonSchema>(
    name: &str,
    description: &str,
    risk: RiskLevel,
    required_capabilities: Vec<Capability>,
    returns_untrusted_data: bool,
) -> ToolMetadata {
    ToolMetadata {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema: serde_json::to_value(schemars::schema_for!(T))
            .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
        risk,
        required_capabilities,
        returns_untrusted_data,
    }
}

/// Capabilities a plan names that the tool's manifest does not declare.
///
/// Returned as sorted, deduplicated `domain.action` names, the same form as
/// [`ToolMetadata::capability_names`]. A non-empty answer means the catalogue a
/// policy author wrote rules against is out of date with what the tool really
/// does. It is not a refusal: the policy engine evaluates the plan whatever the
/// manifest says, so drift is a documentation fault to surface, not a hole to
/// close here.
#[must_use]
pub fn plan_exceeds_manifest(metadata: &ToolMetadata, plan: &ToolPlan) -> Vec<String> {
    let declared = metadata.capability_names();
    let mut undeclared = plan
        .capabilities
        .iter()
        .map(Capability::qualified_name)
        .filter(|name| declared.binary_search(name).is_err())
        .collect::<Vec<_>>();
    undeclared.sort_unstable();
    undeclared.dedup();
    undeclared
}

#[cfg(test)]
mod tests {
    use agentos_core::permission::ResourceRef;
    use agentos_core::trust::DataSource;

    use super::*;

    #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(
        dead_code,
        reason = "deserialised for validation; the value is not read"
    )]
    struct Args {
        path: String,
    }

    #[derive(Debug)]
    struct Dummy(ToolMetadata);

    impl Dummy {
        fn new(name: &str) -> Self {
            Self(metadata_for::<Args>(
                name,
                "a test tool",
                RiskLevel::Low,
                vec![Capability::new("filesystem", "read")],
                true,
            ))
        }
    }

    #[async_trait]
    impl Tool for Dummy {
        fn metadata(&self) -> &ToolMetadata {
            &self.0
        }

        fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
            let _: Args = parse_arguments(&self.0.name, arguments)?;
            Ok(arguments.clone())
        }

        async fn plan(
            &self,
            _arguments: &serde_json::Value,
            _context: &ToolContext,
        ) -> Result<ToolPlan, ToolError> {
            Ok(ToolPlan::new(RiskLevel::Low, "does nothing"))
        }

        async fn execute(
            &self,
            _arguments: serde_json::Value,
            _context: &ToolContext,
            _cancel: CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::text(DataSource::Runtime, "ok"))
        }
    }

    #[test]
    fn registry_lookup_and_filtering() {
        let mut registry = ToolRegistry::new();
        assert!(registry.is_empty());

        registry.register(Arc::new(Dummy::new("filesystem.read")));
        registry.register(Arc::new(Dummy::new("terminal.exec")));

        assert_eq!(registry.len(), 2);
        assert!(registry.get("filesystem.read").is_some());
        assert!(registry.get("nope").is_none());
        assert_eq!(registry.names(), vec!["filesystem.read", "terminal.exec"]);

        let enabled = vec!["filesystem.read".to_owned()];
        let advertised = registry.metadata_for(&enabled);
        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised[0].name, "filesystem.read");
        assert_eq!(registry.all_metadata().len(), 2);
    }

    #[test]
    fn registering_the_same_name_replaces() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(Dummy::new("a")));
        registry.register(Arc::new(Dummy::new("a")));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn schema_comes_from_the_argument_type() {
        let metadata = metadata_for::<Args>("t", "d", RiskLevel::Low, vec![], false);
        let properties = metadata
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("schema should describe properties");
        assert!(properties.contains_key("path"));
    }

    #[test]
    fn validation_rejects_unknown_fields() {
        // A model that invents an extra argument must be refused, not silently
        // have it dropped: the dropped field might have been the safe one.
        let tool = Dummy::new("t");
        let err = tool
            .validate(&serde_json::json!({"path": "/tmp/x", "sudo": true}))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
        assert_eq!(
            err.outcome(),
            agentos_core::tool::ToolOutcome::InvalidArguments
        );
    }

    #[test]
    fn validation_rejects_wrong_types_and_missing_fields() {
        let tool = Dummy::new("t");
        assert!(tool.validate(&serde_json::json!({"path": 42})).is_err());
        assert!(tool.validate(&serde_json::json!({})).is_err());
        assert!(tool.validate(&serde_json::json!({"path": "/ok"})).is_ok());
    }

    #[test]
    fn a_plan_within_the_manifest_exceeds_nothing() {
        let metadata = Dummy::new("filesystem.read").0;
        let plan = ToolPlan::new(RiskLevel::Low, "read").requiring(
            Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
                path: "/tmp/x".into(),
            }),
        );
        assert!(plan_exceeds_manifest(&metadata, &plan).is_empty());
    }

    #[test]
    fn a_plan_beyond_the_manifest_names_each_undeclared_capability_once() {
        let metadata = Dummy::new("filesystem.read").0;
        let plan = ToolPlan::new(RiskLevel::Low, "read, then quietly more")
            .requiring(Capability::new("filesystem", "read"))
            .requiring(Capability::new("terminal", "exec"))
            .requiring(Capability::new("network", "request"))
            .requiring(Capability::new("terminal", "exec"));
        assert_eq!(
            plan_exceeds_manifest(&metadata, &plan),
            vec!["network.request", "terminal.exec"]
        );
    }
}
