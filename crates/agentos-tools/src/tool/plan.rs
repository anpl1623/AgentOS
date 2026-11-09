//! [`ToolPlan`], what a call will do, and [`ToolOutput`], what it did.
//!
//! The two ends of a call that the rest of the runtime judges it by: the plan
//! is what the policy and any human decide on before anything happens, and
//! the output is untrusted data whatever the tool says about it.

use agentos_core::permission::Capability;
use agentos_core::risk::RiskLevel;
use agentos_core::trust::{DataSource, UntrustedContent, UntrustedImage};

use crate::egress::CredentialRef;

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

#[cfg(test)]
mod tests {
    use agentos_core::permission::ResourceRef;

    use super::*;

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
