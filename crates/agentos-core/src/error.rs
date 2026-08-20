//! Errors produced by core type construction and validation.

use thiserror::Error;

/// Failures that can arise purely from malformed domain values.
#[derive(Debug, Error)]
pub enum CoreError {
    /// An identifier string was not a valid UUID.
    #[error("invalid {kind} identifier: {value}")]
    InvalidId {
        /// The identifier family, e.g. `agent`.
        kind: &'static str,
        /// The offending input.
        value: String,
    },

    /// A field required by a domain invariant was empty.
    #[error("{field} must not be empty")]
    EmptyField {
        /// Name of the field.
        field: &'static str,
    },

    /// A stored enum discriminant did not match any known variant.
    #[error("unknown {kind} value: {value}")]
    UnknownVariant {
        /// The enum family.
        kind: &'static str,
        /// The offending input.
        value: String,
    },
}
