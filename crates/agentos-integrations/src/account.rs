//! Bound accounts, and how a call finds the one it acts as.
//!
//! An account is the operator's decision that the runtime may act as somebody
//! at a service: which service, under what label, against which API host, and
//! whether that host may sit on a private network. Every field is the
//! operator's. A tool reads an account; nothing a model writes can create one,
//! change its host or widen its address policy.
//!
//! The token is not here. It is a network credential like any other, stored
//! for the origin of [`Account::host`] under the account's label, and the
//! egress path resolves it at the moment of sending. An account holds the
//! reference ([`Account::credential`]) and never the value.

use std::fmt::Debug;
use std::sync::{Mutex, PoisonError};

use agentos_permissions::normalise_origin;
use agentos_tools::egress::{AddressPolicy, CredentialRef};
use async_trait::async_trait;

use crate::error::IntegrationError;

/// The longest an account label may be.
pub const MAX_LABEL_LEN: usize = 32;

/// Whether `label` is an account label: 1 to [`MAX_LABEL_LEN`] lower-case
/// ASCII letters, digits or `-`.
///
/// Narrower than a credential name on purpose. The label is the credential's
/// name, so it must be one, and it also appears in provenance labels such as
/// `github:work@/repos/...` and in approval cards, where `:`, `@`, `/` or a
/// look-alike capital would make one account read as another.
#[must_use]
pub fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// One bound account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Stable identity of the binding.
    pub id: String,
    /// The integration, e.g. `github`.
    pub integration: String,
    /// The operator's name for the account, unique within the integration.
    pub label: String,
    /// The API base URL every request is built on, e.g.
    /// `https://api.github.com` or `https://ghe.example/api/v3`.
    pub host: String,
    /// Whether the operator allowed this account's host to resolve to a
    /// private-network address, for a self-hosted service on their own
    /// network. Never loopback, link-local or any other range
    /// [`AddressPolicy::AllowPrivateNetwork`] keeps refused.
    pub private_network: bool,
}

impl Account {
    /// The canonical origin of [`Self::host`], which is what the account's
    /// credential is stored for and what a request is checked against.
    ///
    /// # Errors
    ///
    /// [`IntegrationError::Misconfigured`] when the host is not an `http` or
    /// `https` URL with a plain host, or carries a query or fragment.
    pub fn origin(&self) -> Result<String, IntegrationError> {
        if self.host.contains(['?', '#']) {
            return Err(self.misconfigured("it carries a query or fragment"));
        }
        normalise_origin(&self.host).map_err(|error| self.misconfigured(&error.to_string()))
    }

    /// [`Self::host`] without a trailing `/`, ready for a path to be appended.
    ///
    /// # Errors
    ///
    /// As [`Self::origin`], which is checked first so that nothing is ever
    /// built on a host that does not read as one origin.
    pub fn base_url(&self) -> Result<&str, IntegrationError> {
        self.origin()?;
        Ok(self.host.trim_end_matches('/'))
    }

    /// The stored credential this account authenticates with.
    ///
    /// # Errors
    ///
    /// As [`Self::origin`].
    pub fn credential(&self) -> Result<CredentialRef, IntegrationError> {
        Ok(CredentialRef {
            origin: self.origin()?,
            name: self.label.clone(),
        })
    }

    /// The addresses a request for this account may connect to.
    ///
    /// Decided by the operator's flag and nothing else. The egress path keeps
    /// loopback, link-local, multicast and the rest refused under either
    /// answer.
    #[must_use]
    pub const fn address_policy(&self) -> AddressPolicy {
        if self.private_network {
            AddressPolicy::AllowPrivateNetwork
        } else {
            AddressPolicy::Strict
        }
    }

    fn misconfigured(&self, reason: &str) -> IntegrationError {
        IntegrationError::Misconfigured {
            integration: self.integration.clone(),
            label: self.label.clone(),
            message: format!("its host `{}` cannot be used: {reason}", self.host),
        }
    }
}

/// Where integration tools find the accounts the operator has bound.
///
/// Tools hold one directory per integration and ask it on every call, rather
/// than being built once per account: listing the tool catalogue must not read
/// a database, and binding a second account must not change what is
/// registered.
#[async_trait]
pub trait AccountDirectory: Send + Sync + Debug {
    /// Every account bound for `integration`, sorted by label.
    async fn accounts_for(&self, integration: &str) -> Vec<Account>;

    /// The account a call acts as.
    ///
    /// With a label, the account bound under it. Without one, the only account
    /// bound, or [`IntegrationError::Ambiguous`] when there are several and
    /// [`IntegrationError::NotBound`] when there are none.
    ///
    /// # Errors
    ///
    /// As above, and [`IntegrationError::UnknownAccount`] for a label that is
    /// not bound.
    async fn resolve(
        &self,
        integration: &str,
        label: Option<&str>,
    ) -> Result<Account, IntegrationError> {
        choose(integration, label, self.accounts_for(integration).await)
    }

    /// Note that a call acted as `account`, for the operator's "last used"
    /// column. Best effort: a directory that cannot record it says so in its
    /// own log and the call is unaffected.
    async fn record_use(&self, _account: &Account) {}
}

/// The account `label` names among `accounts`, or the only one when it names
/// none.
///
/// The sole-account case is the quiet one: an agent with one GitHub token is
/// not made to name it on every call. An agent with two is never allowed to
/// guess, since the wrong guess posts as the wrong person.
///
/// # Errors
///
/// [`IntegrationError::NotBound`], [`IntegrationError::Ambiguous`] or
/// [`IntegrationError::UnknownAccount`].
pub fn choose(
    integration: &str,
    label: Option<&str>,
    mut accounts: Vec<Account>,
) -> Result<Account, IntegrationError> {
    accounts.retain(|account| account.integration == integration);
    if let Some(label) = label {
        return accounts
            .into_iter()
            .find(|account| account.label == label)
            .ok_or_else(|| IntegrationError::UnknownAccount {
                integration: integration.to_owned(),
                label: label.to_owned(),
            });
    }
    match accounts.len() {
        0 => Err(IntegrationError::NotBound {
            integration: integration.to_owned(),
        }),
        1 => Ok(accounts.remove(0)),
        _ => {
            let mut labels: Vec<String> = accounts.into_iter().map(|a| a.label).collect();
            labels.sort_unstable();
            Err(IntegrationError::Ambiguous {
                integration: integration.to_owned(),
                labels,
            })
        }
    }
}

/// A directory held in memory, for tests and for embedding without a
/// database.
#[derive(Debug, Default)]
pub struct InMemoryDirectory {
    accounts: Mutex<Vec<Account>>,
    used: Mutex<Vec<String>>,
}

impl InMemoryDirectory {
    /// A directory holding `accounts`.
    #[must_use]
    pub fn new(accounts: Vec<Account>) -> Self {
        Self {
            accounts: Mutex::new(accounts),
            used: Mutex::default(),
        }
    }

    /// Bind another account.
    pub fn insert(&self, account: Account) {
        lock(&self.accounts).push(account);
    }

    /// Unbind the account with `id`, returning it.
    pub fn remove(&self, id: &str) -> Option<Account> {
        let mut accounts = lock(&self.accounts);
        let index = accounts.iter().position(|account| account.id == id)?;
        Some(accounts.remove(index))
    }

    /// The ids of the accounts calls have acted as, in order.
    #[must_use]
    pub fn used(&self) -> Vec<String> {
        lock(&self.used).clone()
    }
}

#[async_trait]
impl AccountDirectory for InMemoryDirectory {
    async fn accounts_for(&self, integration: &str) -> Vec<Account> {
        let mut accounts: Vec<Account> = lock(&self.accounts)
            .iter()
            .filter(|account| account.integration == integration)
            .cloned()
            .collect();
        accounts.sort_by(|a, b| a.label.cmp(&b.label));
        accounts
    }

    async fn record_use(&self, account: &Account) {
        lock(&self.used).push(account.id.clone());
    }
}

/// A lock that survives a panic elsewhere: a list of accounts is still a list.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(label: &str) -> Account {
        Account {
            id: format!("id-{label}"),
            integration: "github".into(),
            label: label.into(),
            host: "https://api.github.com".into(),
            private_network: false,
        }
    }

    #[tokio::test]
    async fn one_account_is_used_without_being_named() {
        let directory = InMemoryDirectory::new(vec![account("work")]);
        let chosen = directory.resolve("github", None).await.unwrap();
        assert_eq!(chosen.label, "work");
    }

    #[tokio::test]
    async fn two_accounts_must_be_named_and_a_wrong_name_is_not_a_guess() {
        let directory = InMemoryDirectory::new(vec![account("work"), account("personal")]);
        match directory.resolve("github", None).await {
            Err(IntegrationError::Ambiguous { labels, .. }) => {
                assert_eq!(labels, vec!["personal", "work"]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            directory
                .resolve("github", Some("personal"))
                .await
                .unwrap()
                .label,
            "personal"
        );
        assert!(matches!(
            directory.resolve("github", Some("other")).await,
            Err(IntegrationError::UnknownAccount { .. })
        ));
    }

    #[tokio::test]
    async fn none_bound_and_other_integrations_do_not_count() {
        let mut linear = account("work");
        linear.integration = "linear".into();
        let directory = InMemoryDirectory::new(vec![linear]);
        assert!(matches!(
            directory.resolve("github", None).await,
            Err(IntegrationError::NotBound { .. })
        ));
        assert!(directory.accounts_for("github").await.is_empty());
    }

    #[test]
    fn labels_are_narrow() {
        for good in ["work", "a", "acme-bot-2", &"x".repeat(MAX_LABEL_LEN)] {
            assert!(is_label(good), "{good}");
        }
        for bad in [
            "",
            "Work",
            "work.token",
            "work/1",
            "a:b",
            "a@b",
            "a_b",
            "wörk",
            &"x".repeat(MAX_LABEL_LEN + 1),
        ] {
            assert!(!is_label(bad), "{bad}");
        }
    }

    #[test]
    fn the_credential_is_bound_to_the_hosts_origin_and_named_by_the_label() {
        let mut enterprise = account("work");
        enterprise.host = "https://GHE.example:443/api/v3/".into();
        assert_eq!(
            enterprise.base_url().unwrap(),
            "https://GHE.example:443/api/v3"
        );
        let credential = enterprise.credential().unwrap();
        assert_eq!(credential.origin, "https://ghe.example");
        assert_eq!(credential.name, "work");

        for host in [
            "ftp://ghe.example",
            "https://user:pw@ghe.example",
            "https://ghe.example/api?x=1",
            "https://ghe.example/#a",
            "ghe.example",
        ] {
            enterprise.host = host.into();
            assert!(
                matches!(
                    enterprise.credential(),
                    Err(IntegrationError::Misconfigured { .. })
                ),
                "{host}"
            );
        }
    }

    #[test]
    fn only_the_operators_flag_widens_the_address_policy() {
        let mut account = account("work");
        assert_eq!(account.address_policy(), AddressPolicy::Strict);
        account.private_network = true;
        assert_eq!(account.address_policy(), AddressPolicy::AllowPrivateNetwork);
    }
}
