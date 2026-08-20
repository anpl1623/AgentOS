//! Errors from policy loading and path resolution.

use std::path::PathBuf;

use thiserror::Error;

/// A policy document could not be turned into a usable [`crate::Policy`].
#[derive(Debug, Error)]
pub enum PolicyError {
    /// The YAML was not valid.
    #[error("policy is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    /// A resource pattern could not be compiled.
    #[error("invalid pattern `{pattern}` in rule `{rule}`: {source}")]
    Pattern {
        /// The offending pattern.
        pattern: String,
        /// The rule it appeared in.
        rule: String,
        /// The underlying glob error.
        #[source]
        source: globset::Error,
    },

    /// A filesystem root in the policy could not be resolved.
    #[error("filesystem root `{path}` in rule `{rule}` could not be resolved: {source}")]
    Root {
        /// The offending path.
        path: PathBuf,
        /// The rule it appeared in.
        rule: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A `~` path was used but the home directory is unknown.
    #[error("cannot expand `~` in `{path}`: home directory is unknown")]
    NoHomeDirectory {
        /// The offending path.
        path: String,
    },

    /// The document was structurally valid but semantically wrong.
    #[error("invalid policy: {0}")]
    Invalid(String),
}

/// A path could not be safely resolved, or escaped its sandbox.
#[derive(Debug, Error)]
pub enum PathError {
    /// Relative paths are rejected outright; the caller must anchor them.
    #[error("path must be absolute: {0}")]
    NotAbsolute(PathBuf),

    /// A `..` component remained in a portion of the path that does not exist,
    /// where it cannot be resolved safely.
    #[error("path contains an unresolvable `..` component: {0}")]
    UnresolvableTraversal(PathBuf),

    /// The path resolved to a location outside every allowed root.
    ///
    /// This is the symlink-escape and `..`-traversal outcome.
    #[error("path `{resolved}` is outside every allowed root")]
    OutsideSandbox {
        /// What the caller asked for.
        requested: PathBuf,
        /// Where it actually pointed after resolution.
        resolved: PathBuf,
    },

    /// The filesystem could not be queried.
    #[error("cannot resolve `{path}`: {source}")]
    Io {
        /// The path being resolved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}
