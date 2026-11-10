//! What came back, and which bodies are read as text.
//!
//! A body that is not text is not read at all: a model does not need a
//! megabyte of JPEG. A diff or patch is read only when the caller asked for
//! one.

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressResponse {
    /// The status code.
    pub status: u16,
    /// The status code's canonical reason phrase, or empty.
    pub reason: String,
    /// The response headers in the order received, less `Set-Cookie`: a
    /// session a server opens is not one the run should carry anywhere.
    pub headers: Vec<(String, String)>,
    /// The body, or why it was not read.
    pub body: ResponseBody,
    /// Whether any of the body was left unread or cut off.
    pub truncated: bool,
    /// The URL that answered. Redirects are not followed, so it is the URL
    /// that was asked.
    pub final_url: String,
    /// The `Content-Type`, if one was sent.
    pub content_type: Option<String>,
}

impl EgressResponse {
    /// The first value of header `name`, compared case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body text, when the body was read.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match &self.body {
            ResponseBody::Text(text) => Some(text),
            ResponseBody::None | ResponseBody::NotText { .. } | ResponseBody::TooLarge { .. } => {
                None
            }
        }
    }
}

/// What was done with a response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseBody {
    /// A `HEAD` response has none.
    None,
    /// Not text, so not read: a model does not need a megabyte of JPEG.
    NotText {
        /// The declared length, if the server gave one.
        bytes: Option<u64>,
    },
    /// Declared longer than the limit, so not read.
    TooLarge {
        /// The declared length.
        bytes: u64,
        /// The limit it exceeded.
        cap: usize,
    },
    /// Read, redacted against the credential the request sent, and cut to the
    /// limit at a character boundary.
    Text(String),
}

/// Whether a content type is text a model can read.
///
/// A missing type is read as text: most APIs that omit it are answering in
/// JSON or plain text, and the bytes are decoded leniently either way.
pub(crate) fn is_textual(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return true;
    };
    let essence = essence(content_type);
    essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence.as_str(),
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/ecmascript"
                | "application/x-ndjson"
                | "application/x-www-form-urlencoded"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/graphql"
        )
}

/// Whether a body of this type is read, given whether the caller asked for a
/// diff.
pub(super) fn is_readable(content_type: Option<&str>, accept_diff: bool) -> bool {
    is_textual(content_type)
        || (accept_diff
            && content_type.is_some_and(|content_type| {
                let essence = essence(content_type);
                essence.ends_with(".diff")
                    || essence.ends_with(".patch")
                    || matches!(
                        essence.as_str(),
                        "application/x-diff" | "application/x-patch"
                    )
            }))
}

/// The media type without its parameters, lowercased.
fn essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_is_read_only_when_the_caller_asked_for_one() {
        for diff in [
            "application/vnd.github.diff; charset=utf-8",
            "application/vnd.github.v3.diff",
            "application/vnd.github.v3.patch",
            "application/x-diff",
        ] {
            assert!(!is_readable(Some(diff), false), "{diff}");
            assert!(is_readable(Some(diff), true), "{diff}");
        }
        assert!(is_readable(Some("text/x-diff"), false));
        assert!(!is_readable(Some("image/png"), true));
    }
}
