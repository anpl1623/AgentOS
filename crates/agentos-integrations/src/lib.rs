//! Third-party services, as tools.
//!
//! An integration is a [`Tool`](agentos_tools::Tool) like any other. It gets
//! no special path through the pipeline: its calls are validated, planned,
//! authorised against the policy, put to a person when the policy says so, and
//! recorded, in the same order and by the same code as a file read. Its
//! responses are untrusted like any other tool's, labelled with the account
//! and endpoint they came from so the taint they raise says whose data it was.
//! And it never holds a credential. An account names a stored network
//! credential by origin and label; the plan declares it; the pipeline releases
//! it to the egress path at the moment of sending, records that it did, and
//! redacts it from everything that comes back.
//!
//! Every request leaves through [`agentos_tools::egress::Egress`], the one
//! audited way out of the machine. An integration that reaches the network
//! any other way is a defect.
//!
//! What each integration adds is therefore small: argument types, the
//! sentence an approval card shows, the requests, and a narrow projection of
//! the answers. [`github`] is the reference.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod account;
pub mod error;
#[cfg(feature = "github")]
pub mod github;

pub use account::{Account, AccountDirectory, InMemoryDirectory};
pub use error::IntegrationError;
