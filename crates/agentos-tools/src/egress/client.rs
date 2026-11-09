//! The HTTP client a request is sent with, pinned to the addresses that were
//! checked.
//!
//! Redirects, retries, the `Referer` header and the environment's proxy
//! variables are all off, and the client's resolver knows only the vetted
//! addresses of one host, so a second DNS answer cannot move the connection
//! somewhere else.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use super::USER_AGENT;
use crate::error::ToolError;

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
