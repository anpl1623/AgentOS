//! Integration accounts: who the runtime may act as at a third-party service.
//!
//! An account is two things kept apart. Its token is a network credential like
//! any other, stored for the origin of the account's API host under the
//! account's label, so the one egress path resolves it at the moment of
//! sending, records its use and redacts it, as it does for `network.request`.
//! Everything else — the integration, the label, the host, and whether that
//! host may sit on a private network — is a row in the database, and is the
//! operator's alone: it is written by
//! [`Runtime::bind_integration`](crate::Runtime::bind_integration) and by
//! nothing a run can reach. A tool call names an account by label and gets
//! that row's host; no argument can name a host, or widen where the request
//! may connect.
//!
//! The tools themselves are registered once per integration, whatever is
//! bound, and find their account through [`SqliteAccountDirectory`] on every
//! call. Listing the catalogue therefore reads no database, and binding or
//! unbinding an account changes nothing about what is registered: a tool
//! called with nothing bound says so, and names the command that binds one.
//!
//! Binding, unbinding, listing and testing are operator acts and live with
//! the others in [`crate::operator`]. This module holds what they share with
//! the registry.

use std::path::PathBuf;

use agentos_integrations::account::{Account, AccountDirectory, choose, is_label};
use agentos_integrations::error::IntegrationError;
use agentos_permissions::OriginError;
use agentos_persistence::Database;
use agentos_persistence::integrations::IntegrationAccount;
use agentos_tools::ToolError;
use agentos_tools::egress::Url;
use async_trait::async_trait;
use tokio::sync::OnceCell;

use crate::RuntimeError;

/// An integration the runtime ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownIntegration {
    /// Identifier, which is also the policy domain of its tools.
    pub id: &'static str,
    /// Its name as a person writes it.
    pub display_name: &'static str,
    /// The API base URL an account is bound to when the operator gives none.
    pub default_host: &'static str,
    /// The tools it registers.
    pub tools: &'static [&'static str],
}

/// Every integration the runtime ships, in the order a client lists them.
pub const INTEGRATIONS: &[KnownIntegration] = &[KnownIntegration {
    id: "github",
    display_name: "GitHub",
    default_host: "https://api.github.com",
    tools: agentos_integrations::github::TOOL_NAMES,
}];

/// The identifiers of [`INTEGRATIONS`], for a command line's choices.
pub const INTEGRATION_IDS: &[&str] = &["github"];

/// The integration `id` names, or a refusal listing the ones that exist.
///
/// # Errors
///
/// [`RuntimeError::Rejected`] for an integration the runtime does not ship.
pub fn known_integration(id: &str) -> Result<&'static KnownIntegration, RuntimeError> {
    INTEGRATIONS
        .iter()
        .find(|known| known.id == id)
        .ok_or_else(|| {
            RuntimeError::Rejected(format!(
                "unknown integration `{id}`; expected one of {}",
                INTEGRATION_IDS.join(", ")
            ))
        })
}

/// Where a binding would point: the host as it will be stored, the origin
/// its token will be stored for, and the operator's note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingTarget {
    /// The API base URL, as the origin followed by the path, without a
    /// trailing `/`.
    pub host: String,
    /// The canonical origin of `host`.
    pub origin: String,
    /// What the operator noted the token can do, trimmed, or `None`.
    pub scopes: Option<String>,
}

/// The longest note of a token's scopes.
pub const MAX_SCOPES_LEN: usize = 200;

/// Check an integration, a label, a host, the private-network flag and the
/// scopes note for binding, before anything is asked for or written.
///
/// Public so that a client can refuse before it prompts for a token; the
/// runtime checks again when it binds.
///
/// The host is stored as its canonical origin followed by its path. A host
/// typed `HTTPS://GHE.example:443/api/v3` is the same server as
/// `https://ghe.example/api/v3`, and storing the second is what lets the
/// table's own check, the origin the token is stored for and every URL a tool
/// builds agree about it.
///
/// # Errors
///
/// [`RuntimeError::Rejected`] for an integration the runtime does not ship, a
/// label that is not 1 to 32 lower-case letters, digits or `-`, a host that is
/// not an `http` or `https` URL with a plain host and no query or fragment, a
/// private network asked for on the integration's own public host, or a note
/// that is not a list of scopes.
pub fn binding_target(
    integration: &str,
    label: &str,
    host: Option<&str>,
    private_network: bool,
    scopes: Option<&str>,
) -> Result<BindingTarget, RuntimeError> {
    let known = known_integration(integration)?;
    // The label is not quoted back: a refused label is as likely as not the
    // token, pasted into the wrong place.
    if !is_label(label) {
        return Err(RuntimeError::Rejected(
            "an account label is 1 to 32 lower-case letters, digits or `-`".to_owned(),
        ));
    }
    let typed = host
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .unwrap_or(known.default_host)
        .trim_end_matches('/');
    // The account's own reading of its host, so that what is accepted here
    // is exactly what a tool will later be able to build a request on. A
    // refused host is not quoted back either, for the label's reason: it is
    // asked for in the same breath as the token, and a URL with the token
    // written into its userinfo or query is refused here.
    let refused = || {
        RuntimeError::Rejected(format!(
            "the account host cannot be used: {}; it is the API base URL, such as {}",
            host_problem(typed),
            known.default_host
        ))
    };
    let origin = Account {
        id: String::new(),
        integration: integration.to_owned(),
        label: label.to_owned(),
        host: typed.to_owned(),
        private_network: false,
    }
    .origin()
    .map_err(|_| refused())?;
    let path = Url::parse(typed)
        .map_err(|_| refused())?
        .path()
        .trim_end_matches('/')
        .to_owned();
    let host = format!("{origin}{path}");

    // The service's own host is on the public internet, so the flag could
    // only ever widen where its token may be sent: to a private address that
    // name was made to resolve to.
    if private_network && origin == known.default_host {
        return Err(RuntimeError::Rejected(format!(
            "a private network is for a self-hosted server; {} is {}'s own public host, and \
             is bound without one",
            known.default_host, known.display_name
        )));
    }

    Ok(BindingTarget {
        host,
        origin,
        scopes: scopes_note(scopes)?,
    })
}

/// The operator's note of what a token can do, trimmed, or `None` when there
/// is none.
///
/// A note, not a control, and stored in the database and shown on every
/// listing. So it is held to what a list of scopes is written in, lower-case
/// letters, digits, spaces and `:_,./-`, and to [`MAX_SCOPES_LEN`]: the field
/// is asked for beside the token's, and a token pasted into it, which a
/// GitHub token's capitals and length make plain, is refused here rather than
/// kept in plaintext. The note is not quoted back, for the label's reason.
fn scopes_note(scopes: Option<&str>) -> Result<Option<String>, RuntimeError> {
    let Some(scopes) = scopes.map(str::trim).filter(|scopes| !scopes.is_empty()) else {
        return Ok(None);
    };
    let scope_like = scopes.len() <= MAX_SCOPES_LEN
        && scopes.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b' ' | b':' | b'_' | b',' | b'.' | b'/' | b'-')
        });
    if !scope_like {
        return Err(RuntimeError::Rejected(format!(
            "the scopes note is a list of scopes, such as `repo, read:org`: at most \
             {MAX_SCOPES_LEN} lower-case letters, digits, spaces and `:_,./-`"
        )));
    }
    Ok(Some(scopes.to_owned()))
}

/// What is wrong with a host an account cannot be bound to, in words that do
/// not repeat it.
fn host_problem(host: &str) -> &'static str {
    if host.contains(['?', '#']) {
        return "it carries a query or fragment";
    }
    match agentos_permissions::normalise_origin(host) {
        Err(OriginError::UnsupportedScheme { .. }) => "it is not an http or https URL",
        Err(OriginError::NoHost { .. }) => "it names no host",
        Err(OriginError::CredentialsInUrl { .. }) => "it carries credentials",
        Err(OriginError::InvalidHost { .. }) => {
            "its host is not a plain ASCII name or an IP address literal"
        }
        Err(OriginError::InvalidPort { .. }) => "its port is not a number from 1 to 65535",
        Ok(_) => "it does not read as one origin",
    }
}

/// A bound account as the operator sees it.
#[derive(Debug, Clone)]
pub struct BoundAccount {
    /// The row.
    pub account: IntegrationAccount,
    /// The origin its token is stored for, or `None` for a row whose host no
    /// longer reads as one — which only an edit to the database behind the
    /// runtime's back can produce.
    pub origin: Option<String>,
    /// Whether a token is stored behind the row, as a run would look for it.
    ///
    /// `false` is the failure worth showing loudly: the account looks bound
    /// and every call made as it fails. Removing the credential with
    /// `agentos credential remove` is enough to get here.
    pub credential_present: bool,
}

impl BoundAccount {
    /// Whether this account's token is the network credential stored for
    /// `origin` under `name`.
    ///
    /// One secret, reachable from two places: replacing or removing that
    /// credential under `agentos credential` replaces or removes this
    /// account's token, so a client listing credentials says which ones an
    /// account depends on.
    #[must_use]
    pub fn uses_credential(&self, origin: &str, name: &str) -> bool {
        self.account.label == name && self.origin.as_deref() == Some(origin)
    }

    /// The account as a person names it, such as `GitHub account work`.
    #[must_use]
    pub fn describe(&self) -> String {
        let integration = known_integration(&self.account.integration)
            .map_or(self.account.integration.as_str(), |known| {
                known.display_name
            });
        format!("{integration} account {}", self.account.label)
    }
}

/// What one authenticated read against an account's host found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationCheck {
    /// The verdict.
    pub outcome: CheckOutcome,
    /// One line on what was found. Built from the status, the account's own
    /// fields and the egress path's refusals; never from the response body, so
    /// a host that echoes the token back cannot put it here.
    pub detail: String,
}

/// The verdict of an [`IntegrationCheck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The host answered as the service does, and accepted the token.
    Reachable,
    /// The host answered as the service does, and refused the token, or
    /// there was no token to send.
    Unauthorised,
    /// Something answered, or was refused before it could, that is not the
    /// service's API at this address.
    WrongHost,
    /// Nothing answered.
    Unreachable,
}

impl CheckOutcome {
    /// The stable spelling a client shows or matches on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reachable => "reachable",
            Self::Unauthorised => "unauthorised",
            Self::WrongHost => "wrong_host",
            Self::Unreachable => "unreachable",
        }
    }
}

/// The account a row describes.
pub(crate) fn account_of(row: &IntegrationAccount) -> Account {
    Account {
        id: row.id.to_string(),
        integration: row.integration.clone(),
        label: row.label.clone(),
        host: row.host.clone(),
        private_network: row.private_network,
    }
}

/// The bound accounts, read from the runtime's database.
///
/// Either over a database already open, as the runtime holds it, or over the
/// path of one, opened on first use. The second is what lets
/// [`crate::build_registry`] stay free of side effects: listing the catalogue
/// builds this and never asks it anything, and a tool that does ask opens the
/// same database the runtime would.
#[derive(Debug)]
pub struct SqliteAccountDirectory {
    database: OnceCell<Database>,
    path: Option<PathBuf>,
}

impl SqliteAccountDirectory {
    /// Over an open database.
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self {
            database: OnceCell::new_with(Some(database)),
            path: None,
        }
    }

    /// Over the database at `path`, opened the first time an account is
    /// looked up.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            database: OnceCell::new(),
            path: Some(path.into()),
        }
    }

    async fn database(&self) -> Result<&Database, IntegrationError> {
        self.database
            .get_or_try_init(|| async {
                match &self.path {
                    Some(path) => Database::open(path).await.map_err(unreadable),
                    None => Err(unreadable("no database was given")),
                }
            })
            .await
    }

    async fn rows(&self, integration: &str) -> Result<Vec<Account>, IntegrationError> {
        let rows = self
            .database()
            .await?
            .integrations()
            .list_for(integration)
            .await
            .map_err(unreadable)?;
        Ok(rows.iter().map(account_of).collect())
    }
}

#[async_trait]
impl AccountDirectory for SqliteAccountDirectory {
    async fn accounts_for(&self, integration: &str) -> Vec<Account> {
        match self.rows(integration).await {
            Ok(mut accounts) => {
                accounts.sort_by(|a, b| a.label.cmp(&b.label));
                accounts
            }
            Err(error) => {
                tracing::warn!(%error, integration, "could not read the bound accounts");
                Vec::new()
            }
        }
    }

    /// As the trait's, except that a database that cannot be read is said to
    /// be one, rather than read as "nothing is bound" — which would send an
    /// operator off to bind an account they already have.
    async fn resolve(
        &self,
        integration: &str,
        label: Option<&str>,
    ) -> Result<Account, IntegrationError> {
        choose(integration, label, self.rows(integration).await?)
    }

    async fn record_use(&self, account: &Account) {
        // The id came from a row this directory read, so it parses; one that
        // does not is a row nobody can touch, and is reported as such.
        let touched = match (self.database().await, account.id.parse()) {
            (Ok(database), Ok(id)) => database.integrations().touch(id).await.map_err(unreadable),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(unreadable(error)),
        };
        if let Err(error) = touched {
            tracing::warn!(%error, account = %account.label, "could not record an account's use");
        }
    }
}

/// A database failure, as an integration reports it: a failure of the call,
/// not a fact about what is bound.
fn unreadable(error: impl std::fmt::Display) -> IntegrationError {
    IntegrationError::Transport(ToolError::Failed(format!(
        "the bound accounts could not be read: {error}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_uses_the_credential_at_its_origin_and_label_and_no_other() {
        let bound = BoundAccount {
            account: IntegrationAccount::new(
                "github",
                "work",
                "https://ghe.example/api/v3",
                true,
                None,
            ),
            origin: Some("https://ghe.example".to_owned()),
            credential_present: true,
        };
        assert!(bound.uses_credential("https://ghe.example", "work"));
        assert!(!bound.uses_credential("https://ghe.example", "deploy"));
        assert!(!bound.uses_credential("https://api.github.com", "work"));
        assert_eq!(bound.describe(), "GitHub account work");

        // A row whose host reads as no origin uses no credential at all.
        let unreadable = BoundAccount {
            origin: None,
            ..bound
        };
        assert!(!unreadable.uses_credential("https://ghe.example", "work"));
    }

    #[test]
    fn every_shipped_integration_is_offered_by_its_identifier() {
        let ids: Vec<&str> = INTEGRATIONS.iter().map(|known| known.id).collect();
        assert_eq!(ids, INTEGRATION_IDS);
        for known in INTEGRATIONS {
            let target = binding_target(known.id, "default", None, false, None).unwrap();
            assert_eq!(target.host, known.default_host);
        }
    }

    /// The label rule is written twice, once where a tool reads accounts and
    /// once where the repository writes them, because neither crate depends
    /// on the other. A label one accepts and the other refuses is an account
    /// that binds and cannot be used, or the reverse; this is where the two
    /// are made to agree.
    #[test]
    fn the_tools_and_the_repository_agree_on_what_a_label_is() {
        let mut labels: Vec<String> = (0_u8..=127)
            .map(|byte| char::from(byte).to_string())
            .collect();
        labels.extend(
            [
                "",
                "work",
                "work-2",
                "-",
                "Work",
                "wörk",
                "work.token",
                "a:b",
                "a@b",
                "a/b",
                "a b",
                "a*",
            ]
            .map(str::to_owned),
        );
        labels.extend(["x".repeat(32), "x".repeat(33)]);
        for label in &labels {
            assert_eq!(
                is_label(label),
                agentos_persistence::integrations::is_account_label(label),
                "{label:?}"
            );
        }
    }

    #[test]
    fn a_binding_is_checked_before_anything_is_asked_for() {
        let target = binding_target(
            "github",
            "work",
            Some("  https://GHE.example/api/v3/ "),
            true,
            None,
        )
        .unwrap();
        assert_eq!(target.host, "https://ghe.example/api/v3");
        assert_eq!(target.origin, "https://ghe.example");

        // Stored as the origin the token is stored for, followed by the path,
        // however the scheme, host and port were written. The table's check
        // reads the scheme as written, and a binding it would refuse is one
        // whose token was already stored by the time it said so.
        for (typed, stored) in [
            ("HTTPS://api.github.com", "https://api.github.com"),
            ("Https://API.GitHub.com:443/", "https://api.github.com"),
            (
                "HTTP://GHE.example:8080/API/v3",
                "http://ghe.example:8080/API/v3",
            ),
        ] {
            let target = binding_target("github", "work", Some(typed), false, None).unwrap();
            assert_eq!(target.host, stored, "{typed}");
            assert!(target.host.starts_with(&target.origin), "{typed}");
            assert!(
                target.host.starts_with("https://") || target.host.starts_with("http://"),
                "{typed}"
            );
        }

        assert!(binding_target("gitlab", "work", None, false, None).is_err());
        for label in [
            "",
            "Work",
            "work.token",
            "ghp_0123456789abcdef",
            &"x".repeat(33),
        ] {
            let refused = binding_target("github", label, None, false, None)
                .unwrap_err()
                .to_string();
            assert!(refused.contains("account label"), "{refused}");
            if !label.is_empty() {
                assert!(!refused.contains(label), "the label was quoted: {refused}");
            }
        }
        for host in [
            "ftp://ghe.example",
            "ghe.example",
            "https://user:pw@ghe.example",
            "https://ghe.example/api?token=1",
            "https://ghe.example/#x",
        ] {
            assert!(
                binding_target("github", "work", Some(host), false, None).is_err(),
                "{host}"
            );
        }
        // A token pasted into the host field, alone or written into the URL,
        // is refused without being repeated.
        for (host, token) in [
            ("ghp_0123456789abcdefghij", "ghp_0123456789abcdefghij"),
            (
                "https://x:ghp_0123456789abcdef@ghe.example",
                "ghp_0123456789abcdef",
            ),
            (
                "https://ghe.example/api/v3?access_token=ghp_0123456789",
                "ghp_0123456789",
            ),
        ] {
            let refused = binding_target("github", "work", Some(host), false, None)
                .unwrap_err()
                .to_string();
            assert!(refused.contains("account host"), "{refused}");
            assert!(!refused.contains(token), "the host was quoted: {refused}");
        }

        // The private-network flag is for a server of the operator's own. On
        // the service's public host it could only widen where the token goes.
        for host in [
            None,
            Some("https://api.github.com"),
            Some("HTTPS://api.github.com/"),
        ] {
            let refused = binding_target("github", "work", host, true, None)
                .unwrap_err()
                .to_string();
            assert!(refused.contains("self-hosted"), "{host:?}: {refused}");
        }
        assert!(
            binding_target(
                "github",
                "work",
                Some("https://ghe.example/api/v3"),
                true,
                None
            )
            .is_ok()
        );

        // The scopes note is a list of scopes, and a token pasted into it is
        // refused, without being repeated, before one is asked for.
        for note in [
            "repo, read:org",
            "  issues:write, pull_requests:read ",
            "admin:repo_hook",
        ] {
            let target = binding_target("github", "work", None, false, Some(note)).unwrap();
            assert_eq!(target.scopes.as_deref(), Some(note.trim()));
        }
        assert_eq!(
            binding_target("github", "work", None, false, Some("   "))
                .unwrap()
                .scopes,
            None
        );
        for note in [
            "ghp_EXAMPLETOKEN0123456789abcdefABCDEF",
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnop",
            "repo\nX: 1",
            &"repo,".repeat(41),
        ] {
            let refused = binding_target("github", "work", None, false, Some(note))
                .unwrap_err()
                .to_string();
            assert!(refused.contains("scopes note"), "{refused}");
            assert!(
                !refused.contains(note.trim()),
                "the note was quoted: {refused}"
            );
        }
    }
}
