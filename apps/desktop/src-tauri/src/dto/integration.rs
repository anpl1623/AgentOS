//! Integrations and the accounts bound to them.

use agentos_runtime::integrations::{BoundAccount, IntegrationCheck, KnownIntegration};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{at, maybe_at};

/// An account bound to an integration, as the settings screen lists it.
///
/// Its token is a network credential stored under the account's API origin and
/// its label, so it is told apart the way a credential is: by where it is
/// bound and what it is called. There is no hint of the value, for the reason
/// [`NetworkCredentialView`](super::NetworkCredentialView) gives, and no source:
/// a network credential is read from the keychain alone, so there is only one
/// place it could have come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct IntegrationAccountView {
    /// Identity, for unbinding and testing.
    pub id: String,
    /// The operator's name for the account, which a call may use to pick it.
    pub label: String,
    /// The API base URL every request for this account goes to.
    pub host: String,
    /// The origin its token is stored under, which is the host's, or `None`
    /// for a row whose host no longer reads as one.
    pub origin: Option<String>,
    /// Whether the operator let this account reach a private network address,
    /// for a GitHub Enterprise server on their own network.
    pub private_network: bool,
    /// What the operator noted the token can do. A note, not a limit: what an
    /// agent may do is the policy's to decide.
    pub scopes: Option<String>,
    /// Whether the keychain holds a token for the account.
    ///
    /// `false` is a row with nothing behind it: every call made as the account
    /// fails until it is bound again. The screen shows it rather than hiding it,
    /// because removing the credential under Network credentials is enough to
    /// get here.
    pub credential_present: bool,
    /// When it was bound.
    pub created_at: String,
    /// When a call last acted as it.
    pub last_used_at: Option<String>,
}

/// An integration the runtime ships, with the accounts bound to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct IntegrationView {
    /// Identifier, which is also the domain of its tools.
    pub id: String,
    /// Its name as a person writes it.
    pub display_name: String,
    /// The API base URL an account is bound to when none is given.
    pub default_host: String,
    /// Bound accounts, by label.
    pub accounts: Vec<IntegrationAccountView>,
    /// The tools it registers, which are what binding an account lets an agent
    /// with a matching policy use. Shown before binding so an operator can see
    /// what a token would be spent on.
    pub tools: Vec<String>,
}

/// What one authenticated read against an account's host found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct IntegrationTestView {
    /// `reachable`, `unauthorised`, `wrong_host` or `unreachable`.
    pub outcome: String,
    /// One line on what was found, never quoting the token.
    pub detail: String,
}

impl From<&BoundAccount> for IntegrationAccountView {
    fn from(bound: &BoundAccount) -> Self {
        let account = &bound.account;
        Self {
            id: account.id.to_string(),
            label: account.label.clone(),
            host: account.host.clone(),
            origin: bound.origin.clone(),
            private_network: account.private_network,
            scopes: account.scopes.clone(),
            credential_present: bound.credential_present,
            created_at: at(&account.created_at),
            last_used_at: maybe_at(account.last_used_at.as_ref()),
        }
    }
}

impl IntegrationView {
    /// An integration with the accounts bound to it, picked out of every bound
    /// account by integration.
    #[must_use]
    pub fn new(known: &KnownIntegration, bound: &[BoundAccount]) -> Self {
        Self {
            id: known.id.to_owned(),
            display_name: known.display_name.to_owned(),
            default_host: known.default_host.to_owned(),
            accounts: bound
                .iter()
                .filter(|each| each.account.integration == known.id)
                .map(IntegrationAccountView::from)
                .collect(),
            tools: known.tools.iter().map(|&tool| tool.to_owned()).collect(),
        }
    }
}

impl From<&IntegrationCheck> for IntegrationTestView {
    fn from(check: &IntegrationCheck) -> Self {
        Self {
            outcome: check.outcome.as_str().to_owned(),
            detail: check.detail.clone(),
        }
    }
}
