//! What a request is refused for before anything is resolved or sent.
//!
//! Its target must read as one origin to the policy's parser and the HTTP
//! client alike, a credential it names must be bound to that origin, and no
//! header it sets may carry a credential, change its framing or change its
//! method. `network.request` runs the header check as well, to reject bad
//! arguments before the call is priced.

use agentos_permissions::normalise_origin;
use reqwest::header::{HeaderName, HeaderValue};

use super::{CredentialRef, Url};
use crate::error::ToolError;

/// Header names a request may not set, compared case-insensitively.
///
/// Credentials and cookies arrive only by name, through `credential`. The
/// host, the framing and the hop-by-hop headers belong to the client: a request
/// that set its own `Host` or `Transfer-Encoding` could be read one way by the
/// policy and another by the server.
pub const REFUSED_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "proxy-authorization",
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
];

/// Headers that ask a server to treat the request as a different method.
///
/// Refused outright rather than priced: with one of these a `GET` the policy
/// allowed as a read is a `DELETE` by the time it reaches the application.
pub const METHOD_OVERRIDE_HEADERS: &[&str] = &[
    "x-http-method-override",
    "x-http-method",
    "x-method-override",
];

/// The canonical origin of `url`, if the policy's parser and the client's
/// agree on it.
///
/// The policy was asked about the origin a caller computed; the client
/// connects to what `url` says. They are two parsers, and the one outcome that
/// must never happen is the policy seeing one host while the socket reaches
/// another.
pub(super) fn origin_of(url: &Url) -> Result<String, ToolError> {
    let origin = normalise_origin(url.as_str())
        .map_err(|error| ToolError::Failed(format!("cannot send to `{url}`: {error}")))?;
    if url.host_str().is_some_and(|host| host.ends_with('.')) {
        return Err(ToolError::Failed(format!(
            "cannot send to `{url}`: write the host without a trailing dot"
        )));
    }
    if url.origin().ascii_serialization() != origin {
        return Err(ToolError::Failed(format!(
            "`{url}` does not read as one origin: it is `{origin}` to the policy and `{}` to \
             the HTTP client",
            url.origin().ascii_serialization()
        )));
    }
    Ok(origin)
}

/// Refuse a credential stored for one origin on a request to another.
///
/// The store would miss anyway, since it is keyed by origin. This says so
/// before anything is resolved, in words that name both, and does not depend
/// on the store being written that way.
pub(super) fn check_bound(credential: &CredentialRef, origin: &str) -> Result<(), ToolError> {
    let bound = normalise_origin(&credential.origin).unwrap_or_default();
    if bound != origin {
        return Err(ToolError::Denied {
            reason: format!(
                "credential `{}` is bound to {}, and this request goes to {origin}; a \
                 credential is sent only to the origin it was stored for",
                credential.name, credential.origin
            ),
        });
    }
    Ok(())
}

/// Why a header cannot be sent, if it cannot.
///
/// Shared with `network.request`, which reports the same reasons as invalid
/// arguments before the call is priced.
pub(crate) fn check_header(name: &str, value: &str) -> Result<(), String> {
    // Named first, so the refusal says what is wrong rather than that a
    // header failed to parse.
    if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
        return Err(format!(
            "header `{}` contains a line break, which would start another header",
            name.escape_debug()
        ));
    }
    let lower = name.to_ascii_lowercase();
    if REFUSED_HEADERS.contains(&lower.as_str()) {
        return Err(format!(
            "header `{name}` cannot be set; name a stored credential in `credential` to \
             authenticate"
        ));
    }
    if METHOD_OVERRIDE_HEADERS.contains(&lower.as_str()) {
        return Err(format!(
            "header `{name}` would change the request's method after it was authorised; \
             use `method` instead"
        ));
    }
    HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("`{}` is not a valid header name", name.escape_debug()))?;
    HeaderValue::from_str(value).map_err(|_| {
        format!("the value of header `{name}` contains characters a header cannot carry")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_bound_to_the_origin_it_was_stored_for() {
        let work = CredentialRef::new("https://api.github.com", "work");
        assert_eq!(work.resource(), "https://api.github.com/work");
        assert_eq!(work.to_string(), work.resource());
        assert!(check_bound(&work, "https://api.github.com").is_ok());
        // Spelled differently, the same origin.
        let spelled = CredentialRef::new("https://API.github.com:443", "work");
        assert!(check_bound(&spelled, "https://api.github.com").is_ok());
        for elsewhere in [
            "http://api.github.com",
            "https://api.github.com:8443",
            "https://github.com",
            "https://api.github.com.evil.test",
        ] {
            let error = check_bound(&work, elsewhere).unwrap_err();
            assert!(matches!(error, ToolError::Denied { .. }), "{elsewhere}");
        }
    }
}
