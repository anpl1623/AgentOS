//! Errors from policy loading, path resolution and origin parsing.

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
        /// Why it could not be compiled.
        #[source]
        source: PatternError,
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

/// A resource pattern could not be compiled.
#[derive(Debug, Error)]
pub enum PatternError {
    /// The glob syntax was malformed.
    #[error(transparent)]
    Glob(#[from] globset::Error),

    /// An origin pattern could not be canonicalised.
    ///
    /// Refused at compile time because the alternative is a rule that compiles,
    /// looks right, and never matches: `https://example.com/app` names a path
    /// no origin has, and `example.com` names no scheme.
    #[error(
        "{reason}; an origin is written `scheme://host[:port]`, such as \
         `https://*.example.com` or `http://localhost:*`"
    )]
    Origin {
        /// What was wrong with it.
        reason: String,
    },
}

/// A URL could not be reduced to an origin.
///
/// Every variant carries the URL as given, so the refusal an agent sees names
/// the value it supplied rather than a normalised form of it.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OriginError {
    /// The scheme is not `http` or `https`, or there is no scheme.
    #[error("`{url}` is not an http or https URL")]
    UnsupportedScheme {
        /// The offending value.
        url: String,
    },

    /// There is nothing between the scheme and the path.
    #[error("`{url}` has no host")]
    NoHost {
        /// The offending value.
        url: String,
    },

    /// The URL carries a username or password.
    #[error("`{url}` carries credentials; a URL with userinfo is refused")]
    CredentialsInUrl {
        /// The offending value.
        url: String,
    },

    /// The host is not a plain ASCII name or address literal.
    #[error(
        "`{url}` has a host that is not a plain ASCII name, a dotted IPv4 address or a \
         bracketed IPv6 address"
    )]
    InvalidHost {
        /// The offending value.
        url: String,
    },

    /// The port is missing after its colon, or is not 1-65535.
    #[error("`{url}` has a port that is not a number from 1 to 65535")]
    InvalidPort {
        /// The offending value.
        url: String,
    },
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
