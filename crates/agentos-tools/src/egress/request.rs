//! A request as its caller planned it, and the reference to a stored
//! credential it may carry.
//!
//! Plain data: building one sends and resolves nothing, and a credential
//! appears here only by origin and name, so a request can be printed, planned
//! and recorded freely.

use std::fmt;
use std::time::Duration;

use super::{DEFAULT_MAX_BODY_BYTES, DEFAULT_REQUEST_TIMEOUT, Method, Url};

/// A stored credential, by the origin it is bound to and its name.
///
/// A reference, never a value: it can be printed, planned and recorded, and it
/// is turned into a secret only inside [`Egress::send`], through the run's
/// resolver, at the moment of sending.
///
/// [`Egress::send`]: super::Egress::send
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialRef {
    /// The canonical origin, `scheme://host[:port]`, it was stored for.
    pub origin: String,
    /// The name it was stored under.
    pub name: String,
}

impl CredentialRef {
    /// A reference to credential `name` at `origin`.
    #[must_use]
    pub fn new(origin: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            name: name.into(),
        }
    }

    /// `{origin}/{name}`, the form a `network.credential` resource and the
    /// pipeline's release check use.
    #[must_use]
    pub fn resource(&self) -> String {
        format!("{}/{}", self.origin, self.name)
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.origin, self.name)
    }
}

/// One request, as its caller planned it.
#[derive(Debug, Clone)]
pub struct EgressRequest {
    /// Where it goes. `http` or `https`, with no userinfo.
    pub url: Url,
    /// The method.
    pub method: Method,
    /// Headers beyond those the client sets. None of [`REFUSED_HEADERS`] or
    /// [`METHOD_OVERRIDE_HEADERS`] may appear.
    ///
    /// [`REFUSED_HEADERS`]: super::REFUSED_HEADERS
    /// [`METHOD_OVERRIDE_HEADERS`]: super::METHOD_OVERRIDE_HEADERS
    pub headers: Vec<(String, String)>,
    /// The body, if any.
    pub body: Option<String>,
    /// The stored credential to authenticate with, if any.
    pub credential: Option<CredentialRef>,
    /// How long the connection and the response may each take. The caller
    /// bounds the call as a whole, as it bounds any other awaited work.
    pub timeout: Duration,
    /// The most of the body to keep. Clamped to `1..=`[`MAX_BODY_BYTES`].
    ///
    /// [`MAX_BODY_BYTES`]: super::MAX_BODY_BYTES
    pub max_bytes: usize,
    /// Read a diff or patch media type as text, and read the first
    /// `max_bytes` of a body declared longer than that rather than refusing it.
    ///
    /// Off by default, since `application/vnd.github.diff` and its kind are
    /// not text by their names; a caller that asked for a diff says so here.
    /// The second half follows from the first: the first part of a diff, cut
    /// where the caller marks it, is still a diff, where the first part of a
    /// JSON document is not one. It is read as any undeclared body is, past
    /// the limit by the length of the credential sent, redacted, then cut, so
    /// a declared length changes how much is kept and never what is redacted.
    pub accept_diff: bool,
}

impl EgressRequest {
    /// A request with no headers, body or credential, and the default budgets.
    #[must_use]
    pub fn new(method: Method, url: Url) -> Self {
        Self {
            url,
            method,
            headers: Vec::new(),
            body: None,
            credential: None,
            timeout: DEFAULT_REQUEST_TIMEOUT,
            max_bytes: DEFAULT_MAX_BODY_BYTES,
            accept_diff: false,
        }
    }

    /// Add a header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set the body.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Authenticate with a stored credential.
    #[must_use]
    pub fn with_credential(mut self, credential: CredentialRef) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Set the time budget.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the most of the body to keep.
    #[must_use]
    pub const fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Read a diff or patch media type as text, and as much of a long one as
    /// fits. See [`Self::accept_diff`].
    #[must_use]
    pub const fn accepting_diff(mut self) -> Self {
        self.accept_diff = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_prints_no_secret_because_it_holds_none() {
        let request = EgressRequest::new(
            Method::GET,
            Url::parse("https://api.github.com/user").unwrap(),
        )
        .with_credential(CredentialRef::new("https://api.github.com", "work"));
        let printed = format!("{request:?}");
        assert!(printed.contains("work"), "{printed}");
        assert!(!printed.contains("Bearer"), "{printed}");
        assert_eq!(request.max_bytes, DEFAULT_MAX_BODY_BYTES);
        assert_eq!(request.timeout, DEFAULT_REQUEST_TIMEOUT);
        assert!(!request.accept_diff);
    }
}
