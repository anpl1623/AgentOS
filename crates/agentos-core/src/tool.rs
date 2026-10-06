//! Tool vocabulary: what a tool advertises, how it is called, what it returns.
//!
//! The [`Tool`](../../agentos_tools/trait.Tool.html) trait itself lives in
//! `agentos-tools`; only the data shapes live here so that persistence, audit
//! and providers can talk about tools without depending on their implementations.

use serde::{Deserialize, Serialize};

use crate::permission::Capability;
use crate::risk::RiskLevel;
use crate::trust::{DataSource, UntrustedContent, UntrustedImage};

/// Everything a tool declares about itself.
///
/// All of it is a claim the tool makes, and nothing here is an authorisation
/// input. The runtime authorises the capabilities a call's plan names, not the
/// ones listed here, and derives taint from where returned bytes came from, not
/// from what the tool says it returns. The model only ever sees `name`,
/// `description` and `input_schema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// Fully-qualified name, `domain.action`, e.g. `filesystem.read`.
    pub name: String,
    /// One-paragraph description shown to the model.
    pub description: String,
    /// JSON Schema for the arguments, generated from the typed argument struct.
    pub input_schema: serde_json::Value,
    /// Baseline risk of invoking this tool at all.
    ///
    /// A tool may raise this per-call based on its arguments (deleting a
    /// directory is riskier than deleting a file), but never lower it.
    pub risk: RiskLevel,
    /// The capabilities this tool may plan: its manifest.
    ///
    /// This is what a policy author reads to know which rules a tool needs, and
    /// what the pipeline compares each plan against to catch a tool reaching
    /// beyond what it advertised. The policy engine evaluates the plan itself,
    /// so an incomplete list here cannot widen what a call is allowed to do.
    pub required_capabilities: Vec<Capability>,
    /// Whether this tool reads the world outside the runtime.
    ///
    /// Advisory catalogue metadata with no authorisation effect: it is shown to
    /// operators choosing what to grant, and nothing in the pipeline reads it.
    /// Taint is derived from the provenance of what a call actually returned,
    /// and from the reading capabilities its plan names, so a tool that sets
    /// this wrongly misleads a listing but does not switch off escalation.
    pub returns_untrusted_data: bool,
}

impl ToolMetadata {
    /// The domain portion of the name (`filesystem` in `filesystem.read`).
    #[must_use]
    pub fn domain(&self) -> &str {
        self.name
            .split_once('.')
            .map_or(self.name.as_str(), |p| p.0)
    }

    /// The action portion of the name (`read` in `filesystem.read`).
    #[must_use]
    pub fn action(&self) -> &str {
        self.name.split_once('.').map_or("", |p| p.1)
    }

    /// The manifest as `domain.action` names, sorted and deduplicated.
    ///
    /// Policy rules match on domain and action, so this is the form a policy
    /// author writes rules against; resources are dropped because a manifest
    /// states what kind of thing a tool does, not where. It remains a claim:
    /// the pipeline checks plans against it and records any excess, but the
    /// enforcement point is the policy engine evaluating the plan.
    #[must_use]
    pub fn capability_names(&self) -> Vec<String> {
        let mut names = self
            .required_capabilities
            .iter()
            .map(Capability::qualified_name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// A model's request to invoke a tool.
///
/// This is untrusted input: `arguments` is whatever the model emitted and has
/// not been validated yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call identifier, echoed back with the result.
    pub id: String,
    /// The tool the model asked for.
    pub tool: String,
    /// Raw arguments as emitted by the model.
    pub arguments: serde_json::Value,
}

impl ToolCall {
    /// Build a tool call.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        tool: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            tool: tool.into(),
            arguments,
        }
    }
}

/// How a tool invocation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    /// The tool ran and produced a result.
    Success,
    /// Arguments failed validation; the tool never ran.
    InvalidArguments,
    /// The policy engine denied the call; the tool never ran.
    Denied,
    /// A human declined the approval request; the tool never ran.
    ApprovalDenied,
    /// The run was cancelled before or during execution.
    Cancelled,
    /// The tool ran and failed.
    Failed,
    /// The tool exceeded its time budget and was killed.
    TimedOut,
}

impl ToolOutcome {
    /// Whether the tool actually executed.
    #[must_use]
    pub const fn executed(self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::TimedOut)
    }

    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::InvalidArguments => "invalid_arguments",
            Self::Denied => "denied",
            Self::ApprovalDenied => "approval_denied",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }
}

/// The outcome of a tool invocation, as fed back to the model.
///
/// `content` is always [`UntrustedContent`]. There is no variant that produces
/// control-plane content, which is what prevents a tool result from ever being
/// read as an instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The call this answers.
    pub call_id: String,
    /// The tool that was invoked.
    pub tool: String,
    /// How it ended.
    pub outcome: ToolOutcome,
    /// The payload, always untrusted.
    pub content: UntrustedContent,
    /// Images the tool produced, always untrusted.
    ///
    /// Separate from `content` because providers transport images as their own
    /// content blocks, and because a model without vision must be given the
    /// text and denied the pixels rather than being given neither.
    #[serde(default)]
    pub images: Vec<UntrustedImage>,
    /// Structured data for the UI and for programmatic consumers.
    ///
    /// Never shown to the model as control-plane content.
    pub structured: Option<serde_json::Value>,
}

impl ToolResult {
    /// A successful result carrying untrusted output.
    #[must_use]
    pub fn success(
        call_id: impl Into<String>,
        tool: impl Into<String>,
        content: UntrustedContent,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            tool: tool.into(),
            outcome: ToolOutcome::Success,
            content,
            images: Vec::new(),
            structured: None,
        }
    }

    /// Attach untrusted images to a result.
    #[must_use]
    pub fn with_images(mut self, images: Vec<UntrustedImage>) -> Self {
        self.images = images;
        self
    }

    /// A failure result. The message is still untrusted: error strings routinely
    /// embed attacker-supplied text such as filenames and HTTP response bodies.
    #[must_use]
    pub fn failure(
        call_id: impl Into<String>,
        tool: impl Into<String>,
        outcome: ToolOutcome,
        message: impl Into<String>,
    ) -> Self {
        let tool = tool.into();
        Self {
            call_id: call_id.into(),
            content: UntrustedContent::new(DataSource::Tool { tool: tool.clone() }, message.into()),
            tool,
            outcome,
            images: Vec::new(),
            structured: None,
        }
    }

    /// Attach structured data for non-model consumers.
    #[must_use]
    pub fn with_structured(mut self, value: serde_json::Value) -> Self {
        self.structured = Some(value);
        self
    }

    /// Whether the invocation succeeded.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self.outcome, ToolOutcome::Success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(name: &str) -> ToolMetadata {
        ToolMetadata {
            name: name.to_owned(),
            description: String::new(),
            input_schema: serde_json::json!({}),
            risk: RiskLevel::Low,
            required_capabilities: vec![],
            returns_untrusted_data: true,
        }
    }

    #[test]
    fn splits_qualified_names() {
        let meta = metadata("filesystem.read");
        assert_eq!(meta.domain(), "filesystem");
        assert_eq!(meta.action(), "read");
    }

    #[test]
    fn handles_unqualified_names() {
        let meta = metadata("noop");
        assert_eq!(meta.domain(), "noop");
        assert_eq!(meta.action(), "");
    }

    #[test]
    fn capability_names_are_sorted_unique_and_unscoped() {
        let mut meta = metadata("filesystem.copy");
        meta.required_capabilities = vec![
            Capability::new("filesystem", "write"),
            Capability::new("filesystem", "read").with_resource(
                crate::permission::ResourceRef::Path {
                    path: "/tmp/a".into(),
                },
            ),
            Capability::new("filesystem", "read"),
        ];
        assert_eq!(
            meta.capability_names(),
            vec!["filesystem.read", "filesystem.write"]
        );
    }

    #[test]
    fn failure_results_are_still_untrusted() {
        let result = ToolResult::failure("c1", "browser.extract", ToolOutcome::Failed, "boom");
        assert!(!result.is_success());
        assert_eq!(
            result.content.source,
            DataSource::Tool {
                tool: "browser.extract".into()
            }
        );
    }

    #[test]
    fn non_executed_outcomes_are_marked() {
        assert!(!ToolOutcome::Denied.executed());
        assert!(!ToolOutcome::InvalidArguments.executed());
        assert!(!ToolOutcome::ApprovalDenied.executed());
        assert!(ToolOutcome::Success.executed());
        assert!(ToolOutcome::Failed.executed());
    }
}
