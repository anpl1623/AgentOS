//! Integration errors.

use agentos_tools::ToolError;
use thiserror::Error;

/// Something an integration could not do.
///
/// None of these carries a credential, and none can: the value is resolved
/// inside the egress path and never handed to the code that builds these. The
/// text of a remote error does reach some of them, already redacted by that
/// path and redacted again by the pipeline before it is recorded.
#[derive(Debug, Error)]
pub enum IntegrationError {
    /// No account is bound for the integration.
    #[error(
        "no {integration} account is bound; the operator binds one with \
         `agentos integration add {integration}`"
    )]
    NotBound {
        /// The integration, e.g. `github`.
        integration: String,
    },

    /// Several accounts are bound and the call did not say which.
    #[error(
        "{} {integration} accounts are bound ({}); name one in `account`",
        labels.len(),
        labels.join(", ")
    )]
    Ambiguous {
        /// The integration.
        integration: String,
        /// Every bound label, sorted.
        labels: Vec<String>,
    },

    /// The call named an account that is not bound.
    #[error(
        "no {integration} account is labelled `{label}`; `agentos integration list` shows \
         the bound ones"
    )]
    UnknownAccount {
        /// The integration.
        integration: String,
        /// The label the call gave.
        label: String,
    },

    /// A bound account cannot be used as it stands.
    #[error("the {integration} account `{label}` is misconfigured: {message}")]
    Misconfigured {
        /// The integration.
        integration: String,
        /// The account's label.
        label: String,
        /// What is wrong, and with which field.
        message: String,
    },

    /// The service answered, and the answer was a refusal or an error.
    #[error("{message}")]
    Api {
        /// The HTTP status.
        status: u16,
        /// What happened, in words a model or an operator can act on.
        message: String,
    },

    /// The service answered with something that is not what it documents.
    #[error("the response could not be read: `{field}` was missing or malformed")]
    Malformed {
        /// What was wrong with it.
        field: String,
    },

    /// The request could not be made: refused by the egress path, or failed in
    /// transit.
    ///
    /// Carried whole rather than flattened to text, so a refusal to connect
    /// stays a refusal in the audit record.
    #[error(transparent)]
    Transport(#[from] ToolError),
}

impl From<IntegrationError> for ToolError {
    fn from(error: IntegrationError) -> Self {
        match error {
            // Recoverable by the operator, not by retrying. Reported as an
            // argument problem because the `account` argument, or its absence,
            // is what the model can change.
            IntegrationError::NotBound { ref integration }
            | IntegrationError::Ambiguous {
                ref integration, ..
            }
            | IntegrationError::UnknownAccount {
                ref integration, ..
            } => Self::invalid(integration.clone(), error.to_string()),
            IntegrationError::Transport(inner) => inner,
            IntegrationError::Misconfigured { .. }
            | IntegrationError::Api { .. }
            | IntegrationError::Malformed { .. } => Self::Failed(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::tool::ToolOutcome;

    use super::*;

    #[test]
    fn binding_problems_name_the_command_that_fixes_them() {
        let unbound = ToolError::from(IntegrationError::NotBound {
            integration: "github".into(),
        });
        assert_eq!(unbound.outcome(), ToolOutcome::InvalidArguments);
        assert!(
            unbound
                .to_string()
                .contains("agentos integration add github"),
            "{unbound}"
        );

        let ambiguous = ToolError::from(IntegrationError::Ambiguous {
            integration: "github".into(),
            labels: vec!["personal".into(), "work".into()],
        });
        assert_eq!(ambiguous.outcome(), ToolOutcome::InvalidArguments);
        assert!(ambiguous.to_string().contains("personal, work"));
    }

    #[test]
    fn a_transport_refusal_keeps_its_outcome() {
        let refused = ToolError::from(IntegrationError::from(ToolError::Denied {
            reason: "a link-local address".into(),
        }));
        assert_eq!(refused.outcome(), ToolOutcome::Denied);
        let failed = ToolError::from(IntegrationError::Api {
            status: 500,
            message: "boom".into(),
        });
        assert_eq!(failed.outcome(), ToolOutcome::Failed);
    }
}
