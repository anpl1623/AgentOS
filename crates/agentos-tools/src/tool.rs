//! The [`Tool`] trait and the registry.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::Capability;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use agentos_core::trust::{DataSource, UntrustedContent, UntrustedImage};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

use crate::error::ToolError;

/// Default per-call time budget.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Default cap on how much output a single call may feed back to the model.
///
/// A tool that returns a hundred megabytes of attacker-controlled text is a
/// denial-of-service against the context window, and an expensive one.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Default cap on how many images a single call may return.
///
/// One capture answers one question. A tool returning a filmstrip is either a
/// mistake or an attempt to fill the context window, and both are worth
/// stopping at the pipeline rather than at the provider.
pub const DEFAULT_MAX_IMAGES: usize = 4;

/// Everything a tool needs to know about the run it is executing inside.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The agent.
    pub agent_id: AgentId,
    /// The task.
    pub task_id: TaskId,
    /// The run.
    pub run_id: TaskRunId,
    /// Directory that relative paths are resolved against.
    ///
    /// Being in the workspace does not make a path permitted; the policy decides
    /// that. This only determines what a relative path *means*.
    pub workspace: PathBuf,
    /// Per-call time budget.
    pub timeout: Duration,
    /// Cap on returned output.
    pub max_output_bytes: usize,
    /// Cap on how many images one call may return.
    pub max_images: usize,
    /// Longest edge, in pixels, images are resized to fit within.
    pub max_image_edge: u32,
    /// Cap on the encoded size of each returned image.
    pub max_image_bytes: usize,
}

impl ToolContext {
    /// A context with default budgets.
    #[must_use]
    pub fn new(agent_id: AgentId, task_id: TaskId, run_id: TaskRunId, workspace: PathBuf) -> Self {
        Self {
            agent_id,
            task_id,
            run_id,
            workspace,
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_images: DEFAULT_MAX_IMAGES,
            max_image_edge: crate::vision::DEFAULT_MAX_IMAGE_EDGE,
            max_image_bytes: crate::vision::DEFAULT_MAX_IMAGE_BYTES,
        }
    }

    /// Override the time budget.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the output cap.
    #[must_use]
    pub const fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Override the image budgets.
    #[must_use]
    pub const fn with_image_budget(
        mut self,
        max_images: usize,
        max_image_edge: u32,
        max_image_bytes: usize,
    ) -> Self {
        self.max_images = max_images;
        self.max_image_edge = max_image_edge;
        self.max_image_bytes = max_image_bytes;
        self
    }
}

/// What a specific invocation will do.
///
/// Produced from *validated* arguments, before anything is executed, so the
/// policy engine and the human approving it are looking at the same facts the
/// tool is about to act on.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolPlan {
    /// Every capability this call needs. All are evaluated; the strictest wins.
    pub capabilities: Vec<Capability>,
    /// Risk of this call, which may exceed the tool's baseline.
    ///
    /// Deleting a directory tree is riskier than deleting one file, and the plan
    /// is where that distinction is made.
    pub risk: RiskLevel,
    /// One line describing what will happen, for approval prompts and traces.
    pub summary: String,
    /// Resources touched, for display.
    pub affected_resources: Vec<String>,
}

impl ToolPlan {
    /// Build a plan.
    #[must_use]
    pub fn new(risk: RiskLevel, summary: impl Into<String>) -> Self {
        Self {
            capabilities: Vec::new(),
            risk,
            summary: summary.into(),
            affected_resources: Vec::new(),
        }
    }

    /// Add a required capability.
    #[must_use]
    pub fn requiring(mut self, capability: Capability) -> Self {
        if let Some(resource) = &capability.resource {
            self.affected_resources.push(resource.to_string());
        }
        self.capabilities.push(capability);
        self
    }

    /// Add a displayed resource that is not itself a capability target.
    #[must_use]
    pub fn affecting(mut self, resource: impl Into<String>) -> Self {
        self.affected_resources.push(resource.into());
        self
    }
}

/// What a tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// The payload. Always untrusted — see [`agentos_core::trust`].
    pub content: UntrustedContent,
    /// Images the model should be shown, always untrusted.
    ///
    /// A tool attaches these only when the call was authorised to send pixels
    /// to a model; the pipeline then enforces the run's image budget over them.
    pub images: Vec<UntrustedImage>,
    /// Structured data for the UI and programmatic consumers, never shown to
    /// the model as control-plane content.
    pub structured: Option<serde_json::Value>,
}

impl ToolOutput {
    /// Wrap text from a source.
    #[must_use]
    pub fn text(source: DataSource, body: impl Into<String>) -> Self {
        Self {
            content: UntrustedContent::new(source, body),
            images: Vec::new(),
            structured: None,
        }
    }

    /// Attach an untrusted image for the model to look at.
    #[must_use]
    pub fn with_image(mut self, image: UntrustedImage) -> Self {
        self.images.push(image);
        self
    }

    /// Attach structured data.
    #[must_use]
    pub fn with_structured(mut self, value: serde_json::Value) -> Self {
        self.structured = Some(value);
        self
    }
}

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

    /// Execute.
    ///
    /// Must honour `cancel` promptly and must not exceed `context.timeout`; the
    /// pipeline enforces the timeout as a backstop, but a tool that leaves a
    /// subprocess running after being cancelled has leaked it.
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

#[cfg(test)]
mod tests {
    use agentos_core::permission::ResourceRef;

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
    fn plans_collect_affected_resources_from_capabilities() {
        let plan = ToolPlan::new(RiskLevel::High, "delete a file")
            .requiring(
                Capability::new("filesystem", "delete").with_resource(ResourceRef::Path {
                    path: "/tmp/x".into(),
                }),
            )
            .affecting("2 KiB");
        assert_eq!(plan.capabilities.len(), 1);
        assert_eq!(plan.affected_resources, vec!["path:/tmp/x", "2 KiB"]);
    }
}
