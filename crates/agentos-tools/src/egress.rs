//! The one audited path out of the machine.
//!
//! Every HTTP request the runtime makes on an agent's behalf leaves through
//! [`Egress::send`]: `network.request` does, and so does every integration.
//! What this module refuses is therefore what all of them refuse, and an
//! integration that reaches the network any other way is a defect.
//!
//! The transport decides nothing about whether a request should be made. That
//! was the policy engine's decision, taken on the caller's plan before the call
//! reached `execute`. What `send` guarantees is that the request it makes is
//! the one that was priced: to the host that was named, at an address that was
//! checked, carrying a credential only to the origin it was stored for, with
//! nothing followed, retried or proxied on the way.
//!
//! # Credentials
//!
//! A request names a stored credential by origin and name, [`CredentialRef`];
//! it never carries one. The value is looked up through the context's
//! [`CredentialResolver`] at the moment of sending, which inside the pipeline
//! is a resolver that answers only for what the authorised plan named, records
//! `network.credential.used` before it releases the value, and keeps it for
//! redaction. A credential whose origin is not the request's own is refused
//! before anything is resolved or sent, so a caller that builds the wrong pair
//! fails rather than sends. The value is sent as `Authorization: Bearer
//! <value>` in a header marked sensitive, and the response body is redacted
//! against it before it is cut to the caller's limit, so that no cut can leave
//! a piece of it behind. For the same reason a credentialed request cannot ask
//! for a range of the response.
//!
//! # Redirects are not followed
//!
//! A plan names one origin and the engine answered about that origin. A 301 to
//! somewhere else is a second origin and therefore a second authorisation
//! decision, and there is no way to take one from inside `execute`: the engine
//! has already spoken. So a 3xx comes back as an ordinary response carrying its
//! `Location`. Following it silently, carrying the `Authorization` header
//! along, is the one thing the pipeline exists to prevent.
//!
//! # Addresses
//!
//! Before connecting, the host is resolved and *every* address it resolves to
//! is checked, not the first: a name answering with both a public address and
//! `169.254.169.254` is the standard cloud-metadata SSRF, and checking one
//! address is checking the wrong one. Each address is reduced first to the IPv4
//! address it embeds, if it embeds one, so `::ffff:127.0.0.1` and
//! `64:ff9b::a9fe:a9fe` are judged as what they reach. The connection is then
//! pinned to the addresses that were checked: the client is given a resolver
//! that knows only those, so a second DNS answer cannot move the socket
//! somewhere else. That closes rebinding rather than narrowing it.
//!
//! Which addresses count is fixed when the [`Egress`] is built, by an
//! [`AddressPolicy`]. [`AddressPolicy::Strict`] admits public unicast only and
//! is what `network.request` ships with. [`AddressPolicy::AllowPrivateNetwork`]
//! adds the private ranges an operator's own server can live in, for an
//! integration account whose operator said so when binding it; it never adds
//! loopback, link-local, multicast, broadcast or documentation addresses, nor
//! the cloud metadata services that live outside link-local, inside the very
//! ranges it does add. Loopback is admitted only by
//! [`Egress::for_tests_admitting_loopback`], for a test that stands a server up
//! on `127.0.0.1`. None of the three is reachable from anything a run, a policy
//! file or the environment controls. The same goes for proxies: the client
//! ignores the proxy variables in its environment, which would otherwise route
//! every request through whatever they name.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use agentos_permissions::normalise_origin;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

pub use reqwest::{Method, Url};

use crate::error::ToolError;
use crate::tool::{CredentialResolver, Secret, ToolContext, redact_values};

/// Time budget for a request that does not ask for one.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How much of a response body is read when the caller does not say.
pub const DEFAULT_MAX_BODY_BYTES: usize = 256 * 1024;

/// The most of a response body any request may read.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// The user agent every request carries, so a server's logs say what called.
pub const USER_AGENT: &str = concat!("AgentOS/", env!("CARGO_PKG_VERSION"));

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

/// Which addresses a request may connect to, beyond public unicast.
///
/// Fixed when an [`Egress`] is built. For `network.request` it is always
/// [`Self::Strict`]; for an integration account it is decided by a flag only
/// the operator can set, and no tool argument reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddressPolicy {
    /// Public unicast addresses only.
    #[default]
    Strict,
    /// Public addresses and the private ranges an operator's own server can
    /// live in: RFC 1918, IPv6 unique-local and carrier-grade NAT.
    ///
    /// For a self-hosted service such as GitHub Enterprise on the operator's
    /// network. Loopback, unspecified, link-local, multicast, broadcast and
    /// documentation addresses are refused as they are under [`Self::Strict`]:
    /// none of them is where an operator's server lives, and link-local is
    /// where most clouds' metadata services do. The ones that answer inside the
    /// admitted ranges, Alibaba Cloud's `100.100.100.200` and AWS's
    /// `fd00:ec2::/32`, are refused by name.
    AllowPrivateNetwork,
}

/// A stored credential, by the origin it is bound to and its name.
///
/// A reference, never a value: it can be printed, planned and recorded, and it
/// is turned into a secret only inside [`Egress::send`], through the run's
/// resolver, at the moment of sending.
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
    pub headers: Vec<(String, String)>,
    /// The body, if any.
    pub body: Option<String>,
    /// The stored credential to authenticate with, if any.
    pub credential: Option<CredentialRef>,
    /// How long the connection and the response may each take. The caller
    /// bounds the call as a whole, as it bounds any other awaited work.
    pub timeout: Duration,
    /// The most of the body to keep. Clamped to `1..=`[`MAX_BODY_BYTES`].
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

/// The transport, with the address policy it was built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Egress {
    addresses: AddressPolicy,
    loopback: bool,
}

impl Egress {
    /// A transport that connects only where `addresses` admits.
    #[must_use]
    pub const fn new(addresses: AddressPolicy) -> Self {
        Self {
            addresses,
            loopback: false,
        }
    }

    /// A transport that also connects to loopback, for tests.
    ///
    /// A test's server listens on `127.0.0.1`, and this is the only way to
    /// reach it. Nothing in the runtime calls it, and no configuration,
    /// variable or argument leads here; it exists as a constructor so that
    /// every use is a line a reviewer can find. Every other refusal stands,
    /// the metadata address among them.
    #[must_use]
    pub const fn for_tests_admitting_loopback(addresses: AddressPolicy) -> Self {
        Self {
            addresses,
            loopback: true,
        }
    }

    /// The address policy this transport was built with.
    #[must_use]
    pub const fn address_policy(&self) -> AddressPolicy {
        self.addresses
    }

    /// Whether this transport was built for tests and admits loopback.
    #[must_use]
    pub const fn admits_loopback(&self) -> bool {
        self.loopback
    }

    /// Why `address` may not be connected to, or `None` when it may.
    ///
    /// The address is first reduced to the IPv4 address it carries, if it
    /// carries one, so that it is judged by where it actually reaches.
    #[must_use]
    pub fn refusal(&self, address: IpAddr) -> Option<&'static str> {
        let reason = match canonical(address) {
            IpAddr::V4(v4) => ipv4_refusal(v4),
            IpAddr::V6(v6) => ipv6_refusal(v6),
        }?;
        let admitted = match reason {
            LOOPBACK => self.loopback,
            PRIVATE | UNIQUE_LOCAL | SHARED => self.addresses == AddressPolicy::AllowPrivateNetwork,
            _ => false,
        };
        (!admitted).then_some(reason)
    }

    /// Make one request.
    ///
    /// In order: the target and headers are checked, a credential bound to
    /// another origin is refused, the host is resolved and every address
    /// checked, the client is pinned to those addresses, the credential is
    /// resolved through `context` and attached, the request is sent once, and
    /// the body is read to the limit, redacted, and cut.
    ///
    /// Cancellation and the call's overall deadline are the caller's: race
    /// this against them, as any tool races the work it awaits.
    ///
    /// # Errors
    ///
    /// [`ToolError::Denied`] for an address the policy does not admit or a
    /// credential bound to another origin; [`ToolError::Failed`] for a target
    /// or header that cannot be sent, a credential that cannot be had, and a
    /// transport failure; [`ToolError::Io`] for a host that cannot be resolved.
    pub async fn send(
        &self,
        context: &ToolContext,
        request: EgressRequest,
    ) -> Result<EgressResponse, ToolError> {
        let origin = origin_of(&request.url)?;
        for (name, value) in &request.headers {
            check_header(name, value).map_err(ToolError::Failed)?;
        }
        if let Some(credential) = &request.credential {
            check_bound(credential, &origin)?;
            // A slice of a response is a slice of anything it echoes, and a
            // credential echoed a few bytes at a time, one range per request,
            // is never long enough in any one answer to be recognised.
            if let Some((range, _)) = request.headers.iter().find(|(name, _)| {
                name.eq_ignore_ascii_case("range") || name.eq_ignore_ascii_case("if-range")
            }) {
                return Err(ToolError::Failed(format!(
                    "header `{range}` cannot be set on a request that spends a credential: a \
                     slice of a response can carry a slice of the credential"
                )));
            }
        }

        let client = self.pinned_client(&request.url, request.timeout).await?;

        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            // Both were checked above; a failure here would be a bug, and is
            // reported rather than sending the request without them.
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            let value = HeaderValue::from_str(value)
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            headers.append(name, value);
        }
        let mut sent = None;
        if let Some(credential) = &request.credential {
            let (value, secret) = authorization(context, &origin, &credential.name).await?;
            headers.insert(AUTHORIZATION, value);
            sent = Some(secret);
        }

        let head = request.method == Method::HEAD;
        let mut builder = client.request(request.method, request.url).headers(headers);
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let mut response = builder
            .send()
            .await
            .map_err(|error| request_failed(&origin, &error))?;

        let status = response.status();
        let final_url = response.url().to_string();
        let echoed: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "set-cookie" | "set-cookie2"))
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());

        let cap = request.max_bytes.clamp(1, MAX_BODY_BYTES);
        let (body, truncated) = if head {
            (ResponseBody::None, false)
        } else if !is_readable(content_type.as_deref(), request.accept_diff) {
            let bytes = response.content_length();
            (
                ResponseBody::NotText { bytes },
                bytes.is_some_and(|bytes| bytes > 0),
            )
        } else if let Some(length) = response
            .content_length()
            .filter(|length| *length > u64::try_from(cap).unwrap_or(u64::MAX))
            .filter(|_| !request.accept_diff)
        {
            (ResponseBody::TooLarge { bytes: length, cap }, true)
        } else {
            // The cap is the caller's to choose, and a cut through the middle
            // of an echoed credential leaves a piece that no search for the
            // whole value finds. So the body is read past the cap by the
            // length of the credential this request sent, enough to hold it
            // whole wherever it starts before the cut; it is redacted there,
            // and only then cut. The pipeline redacts the result again against
            // everything the run has released, pieces included, for a token
            // an earlier call sent and this one is handed back.
            let margin = sent
                .as_ref()
                .map_or(0, |secret: &Secret| secret.expose().len());
            let limit = cap.saturating_add(margin);
            let mut read = Vec::new();
            let mut more = false;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| request_failed(&origin, &error))?
            {
                let room = limit - read.len();
                if chunk.len() > room {
                    read.extend_from_slice(&chunk[..room]);
                    more = true;
                    break;
                }
                read.extend_from_slice(&chunk);
            }
            let mut text = String::from_utf8_lossy(&read).into_owned();
            if let Some(secret) = &sent
                && let Some(redacted) = redact_values(&text, &[secret.expose()])
            {
                text = redacted;
            }
            let truncated = more || text.len() > cap;
            if text.len() > cap {
                let mut cut = cap;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
            }
            (ResponseBody::Text(text), truncated)
        };

        Ok(EgressResponse {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or("").to_owned(),
            headers: echoed,
            body,
            truncated,
            final_url,
            content_type,
        })
    }

    /// A client that can reach the checked addresses of `url`'s host and
    /// nothing else.
    async fn pinned_client(
        &self,
        url: &Url,
        timeout: Duration,
    ) -> Result<reqwest::Client, ToolError> {
        let host = url
            .host_str()
            .ok_or_else(|| ToolError::Failed(format!("`{url}` has no host")))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| ToolError::Failed(format!("`{url}` has no port")))?;

        let addresses: Vec<SocketAddr> = if let Some(literal) = ip_literal(host) {
            // The literal is the address; there is no name for an answer to
            // change. The resolver below is installed anyway, so that nothing
            // this client does can reach the system's.
            vec![SocketAddr::new(literal, port)]
        } else {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|source| ToolError::io(format!("resolving `{host}`"), source))?
                .collect()
        };
        if addresses.is_empty() {
            return Err(ToolError::Failed(format!(
                "`{host}` resolved to no address"
            )));
        }
        for address in &addresses {
            if let Some(kind) = self.refusal(address.ip()) {
                return Err(ToolError::Denied {
                    reason: format!(
                        "refusing to connect to `{host}`: it reaches {}, which is {kind}, \
                         not a public address",
                        address.ip()
                    ),
                });
            }
        }
        client_for(host, addresses, timeout)
    }
}

/// The canonical origin of `url`, if the policy's parser and the client's
/// agree on it.
///
/// The policy was asked about the origin a caller computed; the client
/// connects to what `url` says. They are two parsers, and the one outcome that
/// must never happen is the policy seeing one host while the socket reaches
/// another.
fn origin_of(url: &Url) -> Result<String, ToolError> {
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
fn check_bound(credential: &CredentialRef, origin: &str) -> Result<(), ToolError> {
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

/// A client that connects only to `addresses`, whatever name it is asked for.
///
/// `host` resolves to exactly those addresses and every other name fails to
/// resolve. Redirects, retries, the `Referer` header and the proxy variables in
/// the environment are all off: each would send something, or send it
/// somewhere, that the plan did not price.
pub(crate) fn client_for(
    host: &str,
    addresses: Vec<SocketAddr>,
    timeout: Duration,
) -> Result<reqwest::Client, ToolError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        // A retried request is a second request the policy priced once.
        .retry(reqwest::retry::never())
        .referer(false)
        .user_agent(USER_AGENT)
        .connect_timeout(timeout)
        .timeout(timeout)
        .dns_resolver(PinnedResolver {
            host: host.to_ascii_lowercase(),
            addresses,
        })
        .build()
        .map_err(|error| ToolError::Failed(format!("could not build an HTTP client: {error}")))
}

/// The `Authorization` header for credential `name` on `origin`, and the
/// value it carries.
///
/// The value is looked up here, at the moment of sending. The header is marked
/// sensitive so the HTTP stack neither indexes nor prints it, and the value is
/// handed back only so the response can be redacted against it before it is
/// cut short.
async fn authorization(
    context: &ToolContext,
    origin: &str,
    name: &str,
) -> Result<(HeaderValue, Secret), ToolError> {
    let resolver: &dyn CredentialResolver = context.credentials.as_deref().ok_or_else(|| {
        ToolError::Failed(format!(
            "credential `{name}` cannot be used: this run has no credential store"
        ))
    })?;
    let secret = resolver.resolve(origin, name).await.ok_or_else(|| {
        ToolError::Failed(format!(
            "no credential named `{name}` is stored for {origin}; a credential is bound to \
             the origin it was stored for"
        ))
    })?;
    let mut value =
        HeaderValue::from_str(&format!("Bearer {}", secret.expose())).map_err(|_| {
            ToolError::Failed(format!(
                "credential `{name}` contains characters a header cannot carry"
            ))
        })?;
    value.set_sensitive(true);
    Ok((value, secret))
}

/// A transport failure, with every cause in its chain.
///
/// The top-level message is rarely the useful one: "error sending request"
/// becomes useful only with "connection refused" after it.
pub(crate) fn request_failed(origin: &str, error: &reqwest::Error) -> ToolError {
    let mut message = format!("request to {origin} failed: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    ToolError::Failed(message)
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
fn is_readable(content_type: Option<&str>, accept_diff: bool) -> bool {
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

/// The host as an address, when the URL wrote one.
fn ip_literal(host: &str) -> Option<IpAddr> {
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    unbracketed.parse().ok()
}

/// Answers name lookups for one host, with the addresses already checked.
///
/// The client is given this in place of the system resolver, so a name it was
/// not built for cannot be resolved at all and the one it was built for
/// resolves only to what was vetted. A server that answers the first lookup
/// with a public address and the second with `127.0.0.1` gets one lookup.
#[derive(Debug)]
pub(crate) struct PinnedResolver {
    pub(crate) host: String,
    pub(crate) addresses: Vec<SocketAddr>,
}

impl reqwest::dns::Resolve for PinnedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let answer: Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> =
            if name.as_str().eq_ignore_ascii_case(&self.host) {
                Ok(Box::new(self.addresses.clone().into_iter()))
            } else {
                Err(Box::new(Unpinned(name.as_str().to_owned())))
            };
        Box::pin(std::future::ready(answer))
    }
}

/// A lookup for a name the client was not pinned to.
#[derive(Debug)]
struct Unpinned(String);

impl fmt::Display for Unpinned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` was not resolved and checked before connecting",
            self.0
        )
    }
}

impl std::error::Error for Unpinned {}

/// The IPv4 address an IPv6 address stands for, if it stands for one.
///
/// IPv4-mapped (`::ffff:a.b.c.d`) and IPv4-compatible (`::a.b.c.d`) addresses
/// are the IPv4 address. IPv4-translated (`::ffff:0:a.b.c.d`), NAT64
/// (`64:ff9b::/96`) and 6to4 (`2002::/16`) addresses reach the IPv4 address
/// they embed through a translator or a relay. The unspecified and loopback
/// addresses are left as IPv6, since `::` and `::1` would otherwise read as
/// `0.0.0.0` and `0.0.0.1`.
#[must_use]
pub fn canonical(address: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = address else {
        return address;
    };
    if v6.is_unspecified() || v6.is_loopback() {
        return address;
    }
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let segments = v6.segments();
    let octets = v6.octets();
    let low = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
    if segments[..6].iter().all(|segment| *segment == 0) {
        return IpAddr::V4(low);
    }
    if segments[..4].iter().all(|segment| *segment == 0)
        && segments[4] == 0xffff
        && segments[5] == 0
    {
        return IpAddr::V4(low);
    }
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|s| *s == 0) {
        return IpAddr::V4(low);
    }
    if segments[0] == 0x2002 {
        return IpAddr::V4(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }
    address
}

const LOOPBACK: &str = "a loopback address";
const PRIVATE: &str = "a private address";
const UNIQUE_LOCAL: &str = "a unique-local address";
const SHARED: &str = "a shared (carrier-grade NAT) address";

/// Named apart from the range it sits in, because it sits in one the private
/// network policy admits: Alibaba Cloud's metadata service answers at
/// `100.100.100.200`, inside carrier-grade NAT, and AWS's IPv6 one at
/// `fd00:ec2::254`, inside unique-local. A refusal that relied on metadata
/// living in link-local would admit both under
/// [`AddressPolicy::AllowPrivateNetwork`].
const CLOUD_METADATA: &str = "a cloud metadata address";

fn ipv4_refusal(address: Ipv4Addr) -> Option<&'static str> {
    let [a, b, c, _] = address.octets();
    if address == Ipv4Addr::new(100, 100, 100, 200) {
        return Some(CLOUD_METADATA);
    }
    Some(match (a, b, c) {
        (0, _, _) => "an unspecified address",
        (127, _, _) => LOOPBACK,
        (10, _, _) | (192, 168, _) => PRIVATE,
        (172, 16..=31, _) => PRIVATE,
        (100, 64..=127, _) => SHARED,
        (169, 254, _) => "a link-local address",
        (192, 0, 0) => "reserved for protocol assignments",
        (192, 0, 2) | (198, 51, 100) | (203, 0, 113) => "a documentation address",
        (192, 88, 99) => "a deprecated relay address",
        (198, 18..=19, _) => "a benchmarking address",
        (224..=239, _, _) => "a multicast address",
        _ if address.is_broadcast() => "the broadcast address",
        (240..=255, _, _) => "a reserved address",
        _ => return None,
    })
}

fn ipv6_refusal(address: Ipv6Addr) -> Option<&'static str> {
    let segments = address.segments();
    Some(if address.is_unspecified() {
        "an unspecified address"
    } else if address.is_loopback() {
        LOOPBACK
    } else if segments[0] == 0xfd00 && segments[1] == 0x0ec2 {
        // The whole of AWS's `fd00:ec2::/32`: the instance metadata service,
        // its DNS resolver and its time server all answer inside it.
        CLOUD_METADATA
    } else if segments[0] & 0xfe00 == 0xfc00 {
        UNIQUE_LOCAL
    } else if segments[0] & 0xffc0 == 0xfe80 {
        "a link-local address"
    } else if segments[0] & 0xffc0 == 0xfec0 {
        "a site-local address"
    } else if segments[0] & 0xff00 == 0xff00 {
        "a multicast address"
    } else if (segments[0] == 0x2001 && segments[1] == 0x0db8) || segments[0] & 0xfff0 == 0x3ff0 {
        "a documentation address"
    } else if segments[0] == 0x2001 && segments[1] < 0x0200 {
        // Teredo, benchmarking, ORCHID and the rest of the IETF's block. A
        // Teredo address hides the IPv4 address it reaches, so it cannot be
        // judged by it.
        "reserved for protocol assignments"
    } else if segments[0] == 0x0100 && segments[1..4].iter().all(|s| *s == 0) {
        "a discard-only address"
    } else if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001 {
        "a local-use NAT64 address"
    } else if segments[0] & 0xe000 != 0x2000 {
        // The ranges above are named so that a refusal says what was reached.
        // This one is the rule: everything outside 2000::/3 is unassigned or
        // assigned to something other than the public internet, and a range
        // nobody has named yet is refused rather than reached.
        "outside the global unicast range"
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(address: &str) -> IpAddr {
        address.parse().unwrap()
    }

    /// Addresses no policy admits, written as themselves and as the IPv6 forms
    /// that reach them.
    const NEVER: &[&str] = &[
        "0.0.0.0",
        "169.254.169.254",
        "169.254.0.1",
        "192.0.2.1",
        "198.51.100.7",
        "203.0.113.9",
        "224.0.0.1",
        "255.255.255.255",
        "240.0.0.1",
        "192.0.0.8",
        "198.18.0.1",
        "::",
        "fe80::1",
        "ff02::1",
        "fec0::1",
        "2001:db8::1",
        "::ffff:169.254.169.254",
        "64:ff9b::a9fe:a9fe",
        "2002:a9fe:a9fe::1",
        "::ffff:0:a9fe:a9fe",
        // Cloud metadata outside link-local, inside ranges the private
        // network policy admits.
        "100.100.100.200",
        "::ffff:100.100.100.200",
        "64:ff9b::6464:64c8",
        "fd00:ec2::254",
        "fd00:ec2::253",
        "fd00:ec2::123",
        "fd00:ec2::23",
    ];

    /// What an operator's own server can live at.
    const PRIVATE_NETWORK: &[&str] = &[
        "10.0.0.1",
        "10.255.255.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "100.64.0.1",
        "100.127.255.254",
        "fc00::1",
        "fd12:3456::1",
        "::ffff:10.0.0.1",
        "::ffff:192.168.1.1",
    ];

    const LOOPBACK_FORMS: &[&str] = &["127.0.0.1", "127.8.9.10", "::1", "::ffff:127.0.0.1"];

    #[test]
    fn strict_admits_public_unicast_only() {
        let strict = Egress::new(AddressPolicy::Strict);
        for address in NEVER.iter().chain(PRIVATE_NETWORK).chain(LOOPBACK_FORMS) {
            assert!(strict.refusal(ip(address)).is_some(), "{address}");
        }
        for address in ["93.184.216.34", "140.82.112.6", "2606:4700::1111"] {
            assert_eq!(strict.refusal(ip(address)), None, "{address}");
        }
    }

    #[test]
    fn the_private_network_policy_admits_the_operators_ranges_and_nothing_else() {
        let private = Egress::new(AddressPolicy::AllowPrivateNetwork);
        for address in PRIVATE_NETWORK {
            assert_eq!(private.refusal(ip(address)), None, "{address}");
        }
        // Not loopback, so a bound account cannot be pointed at a service on
        // the operator's own machine; and not link-local, where the cloud's
        // metadata service answers.
        for address in NEVER.iter().chain(LOOPBACK_FORMS) {
            assert!(private.refusal(ip(address)).is_some(), "{address}");
        }
        // Nor the metadata services that answer inside the ranges it admits,
        // and the refusal says which they are.
        for address in ["100.100.100.200", "::ffff:100.100.100.200", "fd00:ec2::254"] {
            assert_eq!(
                private.refusal(ip(address)),
                Some(CLOUD_METADATA),
                "{address}"
            );
        }
        assert_eq!(private.refusal(ip("93.184.216.34")), None);
    }

    #[test]
    fn loopback_is_admitted_only_by_the_test_constructor() {
        for policy in [AddressPolicy::Strict, AddressPolicy::AllowPrivateNetwork] {
            assert!(!Egress::new(policy).admits_loopback());
            let test = Egress::for_tests_admitting_loopback(policy);
            assert!(test.admits_loopback());
            for address in LOOPBACK_FORMS {
                assert_eq!(test.refusal(ip(address)), None, "{address}");
            }
            for address in NEVER {
                assert!(test.refusal(ip(address)).is_some(), "{address}");
            }
        }
        // Loopback for tests does not bring the private network with it.
        let test = Egress::for_tests_admitting_loopback(AddressPolicy::Strict);
        assert!(test.refusal(ip("10.0.0.1")).is_some());
    }

    #[test]
    fn the_default_transport_is_strict() {
        assert_eq!(AddressPolicy::default(), AddressPolicy::Strict);
        assert_eq!(Egress::default(), Egress::new(AddressPolicy::Strict));
        assert_eq!(Egress::default().address_policy(), AddressPolicy::Strict);
    }

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
