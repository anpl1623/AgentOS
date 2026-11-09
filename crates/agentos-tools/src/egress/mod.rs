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
//!
//! [`CredentialResolver`]: crate::tool::CredentialResolver

mod address;
mod checks;
mod client;
mod request;
mod response;
mod send;

use std::time::Duration;

pub use reqwest::{Method, Url};

pub use address::{AddressPolicy, canonical};
pub use checks::{METHOD_OVERRIDE_HEADERS, REFUSED_HEADERS};
pub use request::{CredentialRef, EgressRequest};
pub use response::{EgressResponse, ResponseBody};

pub(crate) use checks::check_header;
// `network.request`'s tests build a pinned client by hand and judge content
// types the way `send` does; nothing else outside this module needs these.
#[cfg(test)]
pub(crate) use client::{PinnedResolver, client_for, request_failed};
#[cfg(test)]
pub(crate) use response::is_textual;

/// Time budget for a request that does not ask for one.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How much of a response body is read when the caller does not say.
pub const DEFAULT_MAX_BODY_BYTES: usize = 256 * 1024;

/// The most of a response body any request may read.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// The user agent every request carries, so a server's logs say what called.
pub const USER_AGENT: &str = concat!("AgentOS/", env!("CARGO_PKG_VERSION"));

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
}
