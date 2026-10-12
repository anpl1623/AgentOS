//! Requests to the GitHub REST API, through the one egress path.
//!
//! A request is built from three things and nothing else: the account's host,
//! a path assembled from arguments that were validated to a narrow alphabet,
//! and the account's credential reference. The host is never an argument, and
//! the built URL is read back and compared with what was meant before it is
//! sent, so a path that a URL parser would resolve somewhere else is refused
//! rather than followed there.
//!
//! The answer is mapped to the errors an operator or a model can act on. A 404
//! is not split into "absent" and "not visible to this token", because GitHub
//! does not split it, and saying otherwise would invent a fact.

use std::time::Duration;

use agentos_tools::ToolContext;
use agentos_tools::egress::{
    Egress, EgressRequest, EgressResponse, MAX_BODY_BYTES, Method, ResponseBody, Url,
};
use serde::de::DeserializeOwned;

use super::{API_VERSION, DIFF_MEDIA_TYPE, INTEGRATION, JSON_MEDIA_TYPE, MAX_DIFF_BYTES};
use crate::account::Account;
use crate::error::IntegrationError;

/// The most of a server's own error message that is quoted back.
const MAX_SERVER_MESSAGE_CHARS: usize = 300;

/// One request, before it is placed on an account's host.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    method: Method,
    path: String,
    query: Vec<(&'static str, String)>,
    body: Option<serde_json::Value>,
    /// When the call asks for a diff, the most of it to read.
    diff: Option<usize>,
}

impl Call {
    fn new(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            query: Vec::new(),
            body: None,
            diff: None,
        }
    }

    /// A `GET` of `path`.
    pub(crate) fn get(path: String) -> Self {
        Self::new(Method::GET, path)
    }

    /// A `POST` of `body` to `path`.
    pub(crate) fn post(path: String, body: serde_json::Value) -> Self {
        Self::new(Method::POST, path).with_body(body)
    }

    /// A `PATCH` of `body` to `path`.
    pub(crate) fn patch(path: String, body: serde_json::Value) -> Self {
        Self::new(Method::PATCH, path).with_body(body)
    }

    /// A `PUT` of `body` to `path`.
    pub(crate) fn put(path: String, body: serde_json::Value) -> Self {
        Self::new(Method::PUT, path).with_body(body)
    }

    fn with_body(mut self, body: serde_json::Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Add a query parameter. Encoded when the URL is built.
    pub(crate) fn query(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.query.push((name, value.into()));
        self
    }

    /// Ask for the diff media type instead of JSON, reading at most
    /// `max_bytes` of it (and never more than [`MAX_DIFF_BYTES`]).
    pub(crate) fn diff(mut self, max_bytes: usize) -> Self {
        self.diff = Some(max_bytes.clamp(1, MAX_DIFF_BYTES));
        self
    }

    /// The API path, relative to the account's host. Never the query.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// The URL on `account`'s host.
    ///
    /// # Errors
    ///
    /// [`IntegrationError::Misconfigured`] for an unusable host, and
    /// [`IntegrationError::Transport`] when the URL does not read back as the
    /// account's origin and the intended path.
    pub(crate) fn url(&self, account: &Account) -> Result<Url, IntegrationError> {
        let base = account.base_url()?;
        let refuse = |why: &str| {
            IntegrationError::Transport(agentos_tools::ToolError::Failed(format!(
                "refusing to send to `{}{}`: {why}",
                base, self.path
            )))
        };
        if !self.path.starts_with('/') || self.path.contains("//") {
            return Err(refuse("the path is not a sequence of named segments"));
        }
        let mut url = Url::parse(&format!("{base}{}", self.path))
            .map_err(|error| refuse(&error.to_string()))?;
        if !self.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in &self.query {
                pairs.append_pair(name, value);
            }
        }
        // Read back, not assumed. The arguments that reach `path` were checked
        // to an alphabet with no `/`, `?`, `#`, `%` or dot-segment in it, so a
        // parser has nothing to resolve; if it resolved something anyway, the
        // request would go to an endpoint the capability does not describe.
        let base_path = Url::parse(base)
            .map_err(|error| refuse(&error.to_string()))?
            .path()
            .trim_end_matches('/')
            .to_owned();
        if url.path() != format!("{base_path}{}", self.path) {
            return Err(refuse("the path does not read back as written"));
        }
        if agentos_permissions::normalise_origin(url.as_str()).ok() != Some(account.origin()?) {
            return Err(refuse("it is not the account's host"));
        }
        Ok(url)
    }
}

/// `value` percent-encoded for use as one path segment: everything but ASCII
/// letters, digits and `-._~`, so that a `/` in a branch name stays inside
/// its segment.
pub(crate) fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A successful answer's body.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    /// The text read, already redacted by the egress path.
    pub(crate) body: String,
    /// Whether some of it was left unread.
    pub(crate) truncated: bool,
    /// When nothing was read because the declared length was over the
    /// limit, that length.
    pub(crate) unread: Option<u64>,
}

/// Requests made as one account, within one call.
#[derive(Debug)]
pub(crate) struct Client<'a> {
    egress: Egress,
    account: &'a Account,
    context: &'a ToolContext,
    timeout: Duration,
}

impl<'a> Client<'a> {
    /// A client acting as `account`.
    ///
    /// The transport's address policy is the account's, which only the
    /// operator sets. `base` contributes one thing: whether this is a test
    /// that stands its server up on loopback, which is decided where `base`
    /// was built and nowhere a run can reach.
    pub(crate) fn new(
        base: Egress,
        account: &'a Account,
        context: &'a ToolContext,
        timeout: Duration,
    ) -> Self {
        let egress = if base.admits_loopback() {
            Egress::for_tests_admitting_loopback(account.address_policy())
        } else {
            Egress::new(account.address_policy())
        };
        Self {
            egress,
            account,
            context,
            timeout,
        }
    }

    /// The account the requests are made as.
    pub(crate) const fn account(&self) -> &Account {
        self.account
    }

    /// How much text the call may hand back before the pipeline cuts it.
    pub(crate) const fn output_budget(&self) -> usize {
        self.context.max_output_bytes
    }

    /// Send `call` and return the body of a successful answer. `what` names
    /// the thing asked about, for the error a refusal becomes.
    pub(crate) async fn send(&self, call: &Call, what: &str) -> Result<Reply, IntegrationError> {
        let accept = if call.diff.is_some() {
            DIFF_MEDIA_TYPE
        } else {
            JSON_MEDIA_TYPE
        };
        let mut request = EgressRequest::new(call.method.clone(), call.url(self.account)?)
            .with_header("Accept", accept)
            .with_header("X-GitHub-Api-Version", API_VERSION)
            .with_credential(self.account.credential()?)
            .with_timeout(self.timeout)
            .with_max_bytes(call.diff.unwrap_or(MAX_BODY_BYTES));
        if call.diff.is_some() {
            request = request.accepting_diff();
        }
        if let Some(body) = &call.body {
            request = request
                .with_header("Content-Type", "application/json")
                .with_body(body.to_string());
        }
        let response = self.egress.send(self.context, request).await?;
        if !(200..300).contains(&response.status) {
            return Err(refusal(&response, &self.account.label, what));
        }
        Ok(match response.body {
            ResponseBody::Text(body) => Reply {
                body,
                truncated: response.truncated,
                unread: None,
            },
            ResponseBody::TooLarge { bytes, .. } => Reply {
                body: String::new(),
                truncated: true,
                unread: Some(bytes),
            },
            ResponseBody::None | ResponseBody::NotText { .. } => Reply {
                body: String::new(),
                truncated: response.truncated,
                unread: None,
            },
        })
    }

    /// Send `call` and read the answer as `T`.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        call: &Call,
        what: &str,
    ) -> Result<T, IntegrationError> {
        let reply = self.send(call, what).await?;
        if reply.truncated {
            // Half a JSON document is not a shorter answer, it is no answer.
            return Err(IntegrationError::Api {
                status: 200,
                message: format!(
                    "GitHub's answer about {what} is longer than the {MAX_BODY_BYTES}-byte \
                     limit; ask for fewer items"
                ),
            });
        }
        serde_json::from_str(&reply.body).map_err(|error| IntegrationError::Malformed {
            field: format!("the answer about {what} ({error})"),
        })
    }
}

/// The error a non-success answer becomes.
///
/// Built from the status, the account's label and what was asked about. The
/// server's own words are quoted only where they say something the status
/// does not, such as why a merge was refused, and never for 401 and 403,
/// where the one thing a hostile host might echo is what was sent to it.
fn refusal(response: &EgressResponse, label: &str, what: &str) -> IntegrationError {
    let status = response.status;
    let rate_limited = status == 429
        || (status == 403
            && (response.header("retry-after").is_some()
                || response.header("x-ratelimit-remaining") == Some("0")));
    let message = if rate_limited {
        let when = response
            .header("retry-after")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or_else(
                || match response.header("x-ratelimit-reset") {
                    Some(reset) => format!("when the limit resets (at {} Unix time)", reset.trim()),
                    None => "later".to_owned(),
                },
                |seconds| format!("in {seconds} seconds"),
            );
        format!("GitHub is rate-limiting account `{label}`; try again {when}")
    } else {
        match status {
            401 => format!(
                "GitHub did not accept the token bound as account `{label}` (401). It may have \
                 expired or been revoked; the operator can bind a new one with `agentos \
                 integration remove {INTEGRATION} --label {label}` and then `agentos \
                 integration add {INTEGRATION} --label {label}`"
            ),
            403 => format!(
                "GitHub refused account `{label}` access to {what} (403). The token may lack \
                 the scope this needs, or the account may lack access to the repository; \
                 either is the operator's to change"
            ),
            404 => format!(
                "{what} was not found, or account `{label}` cannot see it. GitHub answers 404 \
                 for both, so which one is not known"
            ),
            300..=399 => format!(
                "GitHub answered {what} with a redirect ({status}), which is not followed. A \
                 repository that was renamed or transferred is reached under its new name"
            ),
            _ => match server_message(response) {
                Some(said) => format!("GitHub refused the request about {what} ({status}): {said}"),
                None => format!(
                    "GitHub refused the request about {what} ({status} {})",
                    response.reason
                ),
            },
        }
    };
    IntegrationError::Api { status, message }
}

/// The `message` of a GitHub error body, cut short and quoted, if it has one.
fn server_message(response: &EgressResponse) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Body {
        message: String,
        #[serde(default)]
        errors: Vec<serde_json::Value>,
    }
    let body: Body = serde_json::from_str(response.text()?).ok()?;
    let mut said = body.message;
    // A 422's `message` is "Validation Failed"; the reason is in `errors`.
    for error in &body.errors {
        if let Some(detail) = error.get("message").and_then(serde_json::Value::as_str) {
            said.push_str("; ");
            said.push_str(detail);
        }
    }
    let mut chars = said.chars();
    let head: String = chars.by_ref().take(MAX_SERVER_MESSAGE_CHARS).collect();
    let more = chars.next().is_some();
    Some(format!(
        "\"{}{}\"",
        head.escape_debug(),
        if more { "\u{2026}" } else { "" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(host: &str) -> Account {
        Account {
            id: "1".into(),
            integration: "github".into(),
            label: "work".into(),
            host: host.into(),
            private_network: false,
        }
    }

    fn response(status: u16, headers: &[(&str, &str)], body: &str) -> EgressResponse {
        EgressResponse {
            status,
            reason: String::new(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body: ResponseBody::Text(body.to_owned()),
            truncated: false,
            final_url: String::new(),
            content_type: Some("application/json".into()),
        }
    }

    #[test]
    fn the_url_is_the_accounts_host_and_the_path_as_written() {
        let call = Call::get("/repos/acme/widgets/issues".into())
            .query("state", "open")
            .query("labels", "good first issue,bug&x=1");
        let url = call.url(&account("https://api.github.com")).unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/acme/widgets/issues?state=open&labels=good+first+issue%2Cbug%26x%3D1"
        );
        let enterprise = call.url(&account("https://GHE.example/api/v3/")).unwrap();
        assert_eq!(enterprise.host_str(), Some("ghe.example"));
        assert!(
            enterprise
                .path()
                .starts_with("/api/v3/repos/acme/widgets/issues")
        );
    }

    #[test]
    fn a_path_a_parser_would_resolve_elsewhere_is_refused() {
        for path in [
            "/repos/../user",
            "/repos/a/./b",
            "/repos/%2e%2e/x",
            "//evil.example/x",
            "@evil.example/x",
            "/repos/a\\b",
        ] {
            assert!(
                Call::get(path.into())
                    .url(&account("https://api.github.com"))
                    .is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn a_ref_stays_in_its_segment() {
        assert_eq!(encode_segment("release/0.2"), "release%2F0.2");
        assert_eq!(encode_segment("v1.2_x-y~"), "v1.2_x-y~");
        let call = Call::get(format!(
            "/repos/acme/widgets/commits/{}/check-runs",
            encode_segment("feature/a b")
        ));
        let url = call.url(&account("https://api.github.com")).unwrap();
        assert_eq!(
            url.path(),
            "/repos/acme/widgets/commits/feature%2Fa%20b/check-runs"
        );
    }

    #[test]
    fn statuses_become_errors_that_say_what_is_known() {
        let message = |r: &EgressResponse| match refusal(r, "work", "acme/widgets#412") {
            IntegrationError::Api { message, .. } => message,
            other => panic!("{other:?}"),
        };
        let echoed = r#"{"message":"Bad credentials: ghp_SECRET"}"#;
        let unauthorised = message(&response(401, &[], echoed));
        assert!(unauthorised.contains("account `work`"), "{unauthorised}");
        assert!(!unauthorised.contains("ghp_"), "{unauthorised}");
        let forbidden = message(&response(403, &[], echoed));
        assert!(forbidden.contains("scope"), "{forbidden}");
        assert!(!forbidden.contains("ghp_"), "{forbidden}");

        let missing = message(&response(404, &[], "{}"));
        assert!(missing.contains("cannot see it"), "{missing}");

        let limited = message(&response(429, &[("Retry-After", "42")], "{}"));
        assert!(limited.contains("in 42 seconds"), "{limited}");
        let secondary = message(&response(
            403,
            &[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1700000000"),
            ],
            "{}",
        ));
        assert!(secondary.contains("1700000000"), "{secondary}");

        let invalid = message(&response(
            422,
            &[],
            r#"{"message":"Validation Failed","errors":[{"message":"No commits between main and x"}]}"#,
        ));
        assert!(
            invalid.contains("No commits between main and x"),
            "{invalid}"
        );
    }
}
