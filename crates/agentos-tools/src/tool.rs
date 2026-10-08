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

use crate::egress::CredentialRef;
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

/// The policy, asked about one capability while a call is already executing.
///
/// Authorisation happens once, against the plan, and a plan names what a call
/// will touch before it touches anything. A tool that discovers what it touches
/// as it goes, such as a recursive search, can only name its root, and path
/// rules match by prefix: a policy that allows a directory and denies a
/// directory inside it authorises the root and says nothing about the inner
/// one. The probe lets such a tool ask the same policy about each path it finds,
/// so a narrower deny binds the walk as well as the plan.
///
/// The pipeline supplies one evaluated with exactly the request shape it used to
/// authorise the call. It answers `true` only for an outright allow: a person
/// who approved the call approved it as planned, not each path it later finds.
pub trait PolicyProbe: Send + Sync + fmt::Debug {
    /// Whether the policy allows `capability`, without asking anyone.
    fn permits(&self, capability: &Capability) -> bool;
}

/// The value of a stored credential.
///
/// A secret is the one thing this crate handles that must never be written
/// down, so the type is built to make writing it down awkward: it has no
/// `Display`, no `Serialize`, a `Debug` that prints `[redacted]`, and one
/// accessor, [`Secret::expose`], whose name is what a reviewer greps for. A
/// struct that derives `Debug` and holds one prints nothing of it.
///
/// The bytes are overwritten when the value is dropped. That is best effort and
/// no more: the string the secret was built from, a header value copied out of
/// it and the allocator's own bookkeeping are beyond its reach. What it does
/// guarantee is that no log line, audit record or approval card can render one
/// by accident.
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wrap a credential's value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().into_bytes())
    }

    /// The value itself, for the one place that must send it.
    #[must_use]
    pub fn expose(&self) -> &str {
        // Built only from a `String`, so this never falls back; the fallback
        // is there because a panic here would be worse than an empty header.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Whether the value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl Clone for Secret {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        // Keeps the compiler from proving the zeroed buffer is never read and
        // removing the store as dead.
        std::hint::black_box(&self.0);
    }
}

/// Where a tool finds a stored credential, by the origin it is bound to.
///
/// The origin is part of the lookup, not a field beside it. A run that has been
/// hijacked can name any credential it likes against any host it likes, and if
/// the host is not the one the operator stored the credential for, the lookup
/// simply misses. Binding is the control, not the tool's good intentions.
///
/// Tools never hold an implementation directly. The pipeline hands each call one
/// that answers only for the credentials its authorised plan named, records
/// the spend in the audit log before it hands a value over, and keeps every
/// value it releases, so the pipeline knows what to redact and what to report
/// without the tool having to tell it. Asynchronous for that reason: the
/// record is written, not queued, by the time the tool holds the secret.
#[async_trait]
pub trait CredentialResolver: Send + Sync + fmt::Debug {
    /// The credential `name` stored for `origin`, a canonical
    /// `scheme://host[:port]`, if there is one.
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret>;
}

/// Every credential value released during one run.
///
/// Written by the resolver the pipeline wraps around the run's own, so it is
/// complete whether or not the tool reports what it used; who was given what
/// is the audit chain's to say, and this keeps only the values. It is what
/// output is redacted against: a token a service stored on one call and echoed
/// on a later one is as live as one echoed straight back.
#[derive(Default)]
pub(crate) struct CredentialLedger {
    issued: std::sync::Mutex<Vec<Secret>>,
}

impl CredentialLedger {
    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Secret>> {
        // A panic while holding this lock leaves a list that is still a list;
        // refusing to redact because of it would be the worse failure.
        self.issued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Record a released value.
    pub(crate) fn record(&self, secret: Secret) {
        self.entries().push(secret);
    }

    /// How many values have been released.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries().len()
    }

    /// `text` with every released value replaced by [`REDACTED_CREDENTIAL`],
    /// or `None` when it contains none of them. See [`redact_values`] for what
    /// counts as containing one.
    pub(crate) fn redact(&self, text: &str) -> Option<String> {
        let entries = self.entries();
        let values: Vec<&str> = entries.iter().map(Secret::expose).collect();
        redact_values(text, &values)
    }

    /// Whether nothing has been released.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }
}

impl fmt::Debug for CredentialLedger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialLedger")
            .field("issued", &self.entries().len())
            .finish()
    }
}

/// What a released credential's value is replaced with wherever it reappears.
pub const REDACTED_CREDENTIAL: &str = "[redacted credential]";

/// The shortest run of a credential's bytes that is redacted on its own.
///
/// A value is not only echoed whole. A response cut short, a server that
/// quotes the first part of what it was sent, a range request over an error
/// page: each returns a piece, and a piece of a token is most of a token. So
/// any run of at least this many consecutive bytes of a released value is
/// redacted wherever it appears, as far as it continues to match. A value
/// shorter than this is redacted only whole, since a shorter run would match
/// ordinary text. What can still pass is fewer than this many bytes from one
/// place, which is a guess, not a credential.
pub const MIN_REDACTED_FRAGMENT: usize = 8;

/// `text` with every occurrence of each value, and every run of at least
/// [`MIN_REDACTED_FRAGMENT`] bytes of one, replaced by
/// [`REDACTED_CREDENTIAL`]; `None` when there is nothing to replace.
///
/// Overlapping matches become one replacement, so a credential that contains
/// another is not left with part of itself showing, and a match that would
/// split a character takes the whole character.
pub(crate) fn redact_values(text: &str, values: &[&str]) -> Option<String> {
    let mut spans: Vec<(usize, usize)> = values
        .iter()
        .flat_map(|value| exposures(text, value))
        .collect();
    if spans.is_empty() {
        return None;
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut redacted = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in merged {
        redacted.push_str(&text[at..start]);
        redacted.push_str(REDACTED_CREDENTIAL);
        at = end;
    }
    redacted.push_str(&text[at..]);
    Some(redacted)
}

/// The byte ranges of `text` that show `value` or a long enough run of it,
/// widened to character boundaries.
fn exposures(text: &str, value: &str) -> Vec<(usize, usize)> {
    if value.is_empty() {
        return Vec::new();
    }
    if value.len() < MIN_REDACTED_FRAGMENT {
        return text
            .match_indices(value)
            .map(|(start, found)| (start, start + found.len()))
            .collect();
    }
    let (haystack, needle) = (text.as_bytes(), value.as_bytes());
    let width = MIN_REDACTED_FRAGMENT;
    let mut windows: std::collections::HashMap<&[u8], Vec<usize>> =
        std::collections::HashMap::new();
    for (offset, window) in needle.windows(width).enumerate() {
        windows.entry(window).or_default().push(offset);
    }
    let mut spans = Vec::new();
    let mut at = 0;
    while at + width <= haystack.len() {
        let Some(offsets) = windows.get(&haystack[at..at + width]) else {
            at += 1;
            continue;
        };
        let length = offsets
            .iter()
            .map(|offset| {
                width
                    + haystack[at + width..]
                        .iter()
                        .zip(&needle[offset + width..])
                        .take_while(|(a, b)| a == b)
                        .count()
            })
            .max()
            .unwrap_or(width);
        spans.push((floor_boundary(text, at), ceil_boundary(text, at + length)));
        // Resume inside the tail of this run rather than after it: a value
        // that repeats part of itself can start a second, longer run there.
        at = (at + length + 1).saturating_sub(width).max(at + 1);
    }
    spans
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

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
    /// The policy, for a tool that must check what it finds during execution.
    ///
    /// `None` outside the pipeline. A tool that depends on it refuses to run
    /// without it rather than fall back to what the plan alone authorised.
    pub policy: Option<Arc<dyn PolicyProbe>>,
    /// Stored credentials, for a tool that authenticates as the operator.
    ///
    /// `None` outside the pipeline and when nothing stores credentials. The
    /// runtime sets the store's own resolver here; the pipeline replaces it,
    /// for each call it executes, with one bound to that call's plan (see
    /// [`CredentialResolver`]), and removes it while the call is planned.
    pub credentials: Option<Arc<dyn CredentialResolver>>,
    /// The capabilities the pipeline authorised this call against, resources
    /// and all.
    ///
    /// Empty outside the pipeline's execute step. A plan can name a resource
    /// it read from the world, such as the origin of the page a browser is on,
    /// and the world can move between the plan and the call. A tool that acts
    /// on such a resource compares what it finds with what was authorised, and
    /// refuses when there is nothing here to compare with.
    pub authorised: Vec<Capability>,
    /// Every credential released during the run this context belongs to.
    ///
    /// Shared by every clone, so one context per run means one ledger per run.
    issued: Arc<CredentialLedger>,
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
            policy: None,
            credentials: None,
            authorised: Vec::new(),
            issued: Arc::default(),
        }
    }

    /// Attach the policy probe.
    #[must_use]
    pub fn with_policy(mut self, policy: Arc<dyn PolicyProbe>) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Attach the credential store.
    #[must_use]
    pub fn with_credentials(mut self, credentials: Arc<dyn CredentialResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// The run's ledger of released credentials.
    pub(crate) fn issued(&self) -> &CredentialLedger {
        &self.issued
    }

    /// The run's ledger, to be written to from outside this context.
    pub(crate) fn issued_handle(&self) -> Arc<CredentialLedger> {
        Arc::clone(&self.issued)
    }

    /// This context with no credential store, for planning.
    pub(crate) fn without_credentials(&self) -> Self {
        Self {
            credentials: None,
            ..self.clone()
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
    /// Stored credentials the call will spend on the strength of its other
    /// capabilities, with no `network.credential` of their own.
    ///
    /// For a tool whose grant is itself the grant to act as an account, such as
    /// an integration's `github.issues.write` on one repository: the operator
    /// bound that account to its credential, and a policy that lets the agent
    /// act as it has said everything there is to say about spending it. The
    /// pipeline releases these, and the credentials a `network.credential`
    /// capability names, and nothing else; every release is recorded and
    /// redacted the same way whichever named it. A tool whose arguments choose
    /// the credential, as `network.request`'s do, names it as a capability
    /// instead, so that the policy is asked.
    pub credentials: Vec<CredentialRef>,
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
            credentials: Vec::new(),
        }
    }

    /// Declare a stored credential the call will spend under its other
    /// capabilities. See [`Self::credentials`] for when that is right.
    ///
    /// It is shown with the affected resources, so the person approving the
    /// call sees whose credential it is.
    #[must_use]
    pub fn spending(mut self, credential: CredentialRef) -> Self {
        self.affected_resources
            .push(format!("credential:{}", credential.resource()));
        self.credentials.push(credential);
        self
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

    #[test]
    fn a_secret_prints_as_redacted_wherever_it_is_held() {
        #[derive(Debug)]
        #[allow(dead_code, reason = "printed, not read")]
        struct Holder {
            secret: Secret,
        }
        let holder = Holder {
            secret: Secret::new("hunter2-value"),
        };
        let printed = format!("{holder:?} {:?}", holder.secret.clone());
        assert!(!printed.contains("hunter2"), "{printed}");
        assert_eq!(holder.secret.expose(), "hunter2-value");
    }

    #[test]
    fn the_ledger_redacts_the_longer_of_two_overlapping_values_whole() {
        let ledger = CredentialLedger::default();
        assert_eq!(ledger.redact("nothing released yet"), None);
        ledger.record(Secret::new("abc"));
        ledger.record(Secret::new("abcdef"));
        ledger.record(Secret::new(""));
        assert_eq!(
            ledger.redact("got abcdef and abc").as_deref(),
            Some("got [redacted credential] and [redacted credential]")
        );
        assert_eq!(ledger.redact("nothing here"), None);
        assert!(!format!("{ledger:?}").contains("abc"));
    }

    #[test]
    fn no_run_of_a_released_value_long_enough_to_matter_survives() {
        const TOKEN: &str = "tok_live_9f8e7d6c5b4a39281706f5e4d3c2b1a0";
        let ledger = CredentialLedger::default();
        ledger.record(Secret::new(TOKEN));

        let k = MIN_REDACTED_FRAGMENT;
        let mut pieces = vec![TOKEN.to_owned()];
        // Every prefix and suffix a cut could leave, and a slice from the
        // middle such as a range request returns.
        for cut in k..TOKEN.len() {
            pieces.push(TOKEN[..cut].to_owned());
            pieces.push(TOKEN[TOKEN.len() - cut..].to_owned());
        }
        pieces.push(TOKEN[10..10 + k].to_owned());
        for piece in pieces {
            let text = format!("received \"Bearer {piece}\" [truncated]");
            let redacted = ledger.redact(&text).unwrap_or_else(|| panic!("{piece}"));
            assert_eq!(
                redacted, "received \"Bearer [redacted credential]\" [truncated]",
                "{piece}"
            );
        }

        // Shorter than the floor is ordinary text, and is left alone.
        assert_eq!(ledger.redact(&format!("id {}", &TOKEN[..k - 1])), None);

        // A token echoed twice, back to back, and one split across a
        // multi-byte character, are both covered whole.
        assert_eq!(
            ledger.redact(&format!("{TOKEN}{TOKEN}")).as_deref(),
            Some("[redacted credential]")
        );
        let ledger = CredentialLedger::default();
        ledger.record(Secret::new("pässwörd-çödé-1"));
        assert_eq!(
            ledger.redact("x ässwörd-çö y").as_deref(),
            Some("x [redacted credential] y")
        );
    }
}
