//! Secret storage.
//!
//! API keys and integration credentials live in the operating system's keychain
//! — Keychain on macOS, Credential Manager on Windows, Secret Service on Linux —
//! and never in the AgentOS database, never in a config file, and never in a log
//! line. The database stores a *reference* to a secret; this crate resolves it.
//!
//! [`SecretStore`] is a trait so tests can run without touching the real
//! keychain, and so a future implementation (a hardware token, a remote vault)
//! does not require changing call sites.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use thiserror::Error;

/// The service name AgentOS registers under in the OS keychain.
pub const SERVICE_NAME: &str = "dev.agentos.runtime";

/// Something went wrong talking to the secret store.
#[derive(Debug, Error)]
pub enum SecretError {
    /// No secret is stored under that key.
    #[error("no secret stored for `{key}`")]
    NotFound {
        /// The key that was looked up.
        key: String,
    },

    /// The platform keychain refused or failed.
    #[error("keychain error for `{key}`: {message}")]
    Backend {
        /// The key involved.
        key: String,
        /// Detail from the platform.
        message: String,
    },

    /// The store cannot be written to.
    #[error("the {store} secret store is read-only; cannot store `{key}` there")]
    ReadOnly {
        /// Which store refused.
        store: &'static str,
        /// The key involved.
        key: String,
    },

    /// The store's internal lock was poisoned by a panic elsewhere.
    #[error("secret store lock was poisoned")]
    Poisoned,
}

/// A value read out of the secret store.
///
/// Wraps the string so that `Debug` and `Display` cannot leak it into a log.
/// The only way to see the contents is [`Secret::expose`], which is greppable
/// during review.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self(value)
    }

    /// Read the underlying value.
    ///
    /// Named to be conspicuous: every call site is a place a secret could escape.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A redacted hint safe to show a human, e.g. `sk-a…9f2`.
    ///
    /// Short secrets are fully redacted rather than partially revealed.
    #[must_use]
    pub fn hint(&self) -> String {
        let chars: Vec<char> = self.0.chars().collect();
        if chars.len() < 12 {
            return "•".repeat(8);
        }
        let head: String = chars.iter().take(4).collect();
        let tail: String = chars.iter().skip(chars.len() - 3).collect();
        format!("{head}…{tail}")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Somewhere secrets can be kept.
pub trait SecretStore: Send + Sync + fmt::Debug {
    /// Short name for this store, for `agentos doctor` and error messages.
    fn name(&self) -> &'static str;

    /// Whether this store accepts writes.
    ///
    /// Read-only stores exist — the environment is one — and a chain needs to
    /// know where a `set` can actually go.
    fn is_writable(&self) -> bool {
        true
    }

    /// Read a secret.
    ///
    /// # Errors
    ///
    /// [`SecretError::NotFound`] if absent, [`SecretError::Backend`] on failure.
    fn get(&self, key: &str) -> Result<Secret, SecretError>;

    /// Write a secret, replacing any existing value.
    ///
    /// # Errors
    ///
    /// [`SecretError::Backend`] on failure.
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;

    /// Remove a secret. Removing something absent is not an error.
    ///
    /// # Errors
    ///
    /// [`SecretError::Backend`] on failure.
    fn delete(&self, key: &str) -> Result<(), SecretError>;

    /// Whether a secret exists, without reading it.
    ///
    /// # Errors
    ///
    /// [`SecretError::Backend`] on failure.
    fn contains(&self, key: &str) -> Result<bool, SecretError> {
        match self.get(key) {
            Ok(_) => Ok(true),
            Err(SecretError::NotFound { .. }) => Ok(false),
            Err(other) => Err(other),
        }
    }
}

/// The platform keychain.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringStore;

/// Whether a platform keychain is usable on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainStatus {
    /// A keychain is present and answering.
    Available,
    /// No keychain backend is available.
    ///
    /// Normal on a headless Linux box, in a container, or in CI: there is no
    /// D-Bus session and therefore no Secret Service. It is not an error, it is
    /// a fact about the machine, and AgentOS has to remain usable there.
    Unavailable {
        /// What the platform said.
        reason: String,
    },
}

impl KeychainStatus {
    /// Whether secrets can be stored.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

impl KeyringStore {
    /// Use the platform keychain under the AgentOS service name.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Check whether this machine has a usable keychain.
    ///
    /// Probes by looking up a name that will not exist: "no such entry" proves
    /// the backend is answering, while a failure to even construct the entry
    /// means there is no backend at all.
    #[must_use]
    pub fn status() -> KeychainStatus {
        match Self::entry("__agentos_probe__") {
            Err(SecretError::Backend { message, .. }) => {
                KeychainStatus::Unavailable { reason: message }
            }
            Err(error) => KeychainStatus::Unavailable {
                reason: error.to_string(),
            },
            Ok(entry) => match entry.get_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => KeychainStatus::Available,
                Err(error) => KeychainStatus::Unavailable {
                    reason: error.to_string(),
                },
            },
        }
    }

    fn entry(key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE_NAME, key).map_err(|error| SecretError::Backend {
            key: key.to_owned(),
            message: error.to_string(),
        })
    }
}

impl SecretStore for KeyringStore {
    fn name(&self) -> &'static str {
        "system keychain"
    }

    fn get(&self, key: &str) -> Result<Secret, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Secret::new(value)),
            Err(keyring::Error::NoEntry) => Err(SecretError::NotFound {
                key: key.to_owned(),
            }),
            Err(error) => Err(SecretError::Backend {
                key: key.to_owned(),
                message: error.to_string(),
            }),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|error| SecretError::Backend {
                key: key.to_owned(),
                message: error.to_string(),
            })
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::Backend {
                key: key.to_owned(),
                message: error.to_string(),
            }),
        }
    }
}

/// An in-process store for tests and for `--no-keychain` runs.
///
/// Never persists. Using this in production would mean re-entering every
/// credential on each start, which is the intended deterrent.
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    entries: Mutex<HashMap<String, String>>,
}

impl InMemorySecretStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, String>>, SecretError> {
        self.entries
            .lock()
            .map_err(|_: PoisonError<_>| SecretError::Poisoned)
    }
}

impl SecretStore for InMemorySecretStore {
    fn name(&self) -> &'static str {
        "in-memory"
    }

    fn get(&self, key: &str) -> Result<Secret, SecretError> {
        self.lock()?
            .get(key)
            .map(|value| Secret::new(value.clone()))
            .ok_or_else(|| SecretError::NotFound {
                key: key.to_owned(),
            })
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.lock()?.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.lock()?.remove(key);
        Ok(())
    }
}

/// Provider credentials supplied through the environment.
///
/// Read-only, and the fallback for machines with no keychain: headless servers,
/// containers, CI, WSL. Without it AgentOS is simply unusable there, which is
/// not an acceptable answer for a runtime meant to run unattended work.
///
/// It is a weaker place to keep a credential than a keychain — anything that can
/// read the process environment can read it. Two things limit the damage:
/// `terminal.exec` passes child processes an allowlist that does not include
/// these variables, so an agent cannot read them back out through a subprocess;
/// and nothing in AgentOS writes them anywhere.
///
/// For `provider.anthropic.api_key` it looks at `AGENTOS_ANTHROPIC_API_KEY`
/// first, then the conventional `ANTHROPIC_API_KEY`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvSecretStore;

impl EnvSecretStore {
    /// Read credentials from the environment.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Environment variable names checked for a key, in order.
    #[must_use]
    pub fn variables_for(key: &str) -> Vec<String> {
        let normalised: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();

        let mut names = vec![format!("AGENTOS_{normalised}")];

        // Also honour the conventional name a user probably already exports.
        if let Some(provider) = key
            .strip_prefix("provider.")
            .and_then(|rest| rest.strip_suffix(".api_key"))
        {
            let provider: String = provider
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            names.push(format!("{provider}_API_KEY"));
        }
        names
    }
}

impl SecretStore for EnvSecretStore {
    fn name(&self) -> &'static str {
        "environment"
    }

    fn is_writable(&self) -> bool {
        false
    }

    fn get(&self, key: &str) -> Result<Secret, SecretError> {
        for variable in Self::variables_for(key) {
            if let Ok(value) = std::env::var(&variable)
                && !value.trim().is_empty()
            {
                return Ok(Secret::new(value.trim().to_owned()));
            }
        }
        Err(SecretError::NotFound {
            key: key.to_owned(),
        })
    }

    fn set(&self, key: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::ReadOnly {
            store: self.name(),
            key: key.to_owned(),
        })
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        Err(SecretError::ReadOnly {
            store: self.name(),
            key: key.to_owned(),
        })
    }
}

/// Several stores consulted in order.
///
/// Reads try each store until one answers. Writes go to the first writable
/// store, so configuring a key still lands in the keychain when there is one,
/// and fails with a useful message when there is not.
#[derive(Debug)]
pub struct ChainSecretStore {
    stores: Vec<std::sync::Arc<dyn SecretStore>>,
}

impl ChainSecretStore {
    /// Build a chain.
    #[must_use]
    pub fn new(stores: Vec<std::sync::Arc<dyn SecretStore>>) -> Self {
        Self { stores }
    }

    /// The standard chain: the platform keychain, then the environment.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(vec![
            std::sync::Arc::new(KeyringStore::new()),
            std::sync::Arc::new(EnvSecretStore::new()),
        ])
    }

    /// Find a secret and report which store had it.
    ///
    /// `agentos doctor` uses this to tell an operator where a credential is
    /// actually coming from, which matters when two places disagree.
    #[must_use]
    pub fn locate(&self, key: &str) -> Option<(&'static str, Secret)> {
        for store in &self.stores {
            if let Ok(secret) = store.get(key) {
                return Some((store.name(), secret));
            }
        }
        None
    }

    /// The stores in this chain, in order.
    #[must_use]
    pub fn stores(&self) -> &[std::sync::Arc<dyn SecretStore>] {
        &self.stores
    }
}

impl SecretStore for ChainSecretStore {
    fn name(&self) -> &'static str {
        "chain"
    }

    fn is_writable(&self) -> bool {
        self.stores.iter().any(|store| store.is_writable())
    }

    fn get(&self, key: &str) -> Result<Secret, SecretError> {
        for store in &self.stores {
            match store.get(key) {
                Ok(secret) => return Ok(secret),
                Err(SecretError::NotFound { .. }) => {}
                // A store that cannot answer is not the same as a missing
                // credential, but reporting it here would be the wrong error in
                // the common case: on a machine with no keychain, every lookup
                // would surface "no default store has been set" instead of the
                // actionable "no key is configured". Keychain health is
                // reported once, by `agentos doctor`, where it belongs.
                Err(error) => {
                    tracing::warn!(
                        store = store.name(),
                        %error,
                        "secret store could not be consulted"
                    );
                }
            }
        }
        Err(SecretError::NotFound {
            key: key.to_owned(),
        })
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        for store in &self.stores {
            if store.is_writable() {
                return store.set(key, value);
            }
        }
        Err(SecretError::ReadOnly {
            store: "chain",
            key: key.to_owned(),
        })
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        for store in &self.stores {
            if store.is_writable() {
                return store.delete(key);
            }
        }
        Ok(())
    }
}

/// The keychain key an LLM provider's credential is stored under.
#[must_use]
pub fn provider_key(provider: &str) -> String {
    format!("provider.{provider}.api_key")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn secrets_are_redacted_in_debug_and_display() {
        let secret = Secret::new("sk-super-secret-value".to_owned());
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{secret}"), "<redacted>");
        assert!(!format!("{secret:?} {secret}").contains("super"));
    }

    #[test]
    fn hints_reveal_only_the_ends_of_long_secrets() {
        let secret = Secret::new("sk-ant-api03-abcdef123456".to_owned());
        let hint = secret.hint();
        assert_eq!(hint, "sk-a…456");
        assert!(!hint.contains("api03"));
    }

    #[test]
    fn short_secrets_are_fully_redacted() {
        assert_eq!(Secret::new("short".to_owned()).hint(), "••••••••");
    }

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemorySecretStore::new();
        assert!(!store.contains("k").unwrap());

        store.set("k", "v").unwrap();
        assert_eq!(store.get("k").unwrap().expose(), "v");
        assert!(store.contains("k").unwrap());

        store.set("k", "v2").unwrap();
        assert_eq!(store.get("k").unwrap().expose(), "v2");

        store.delete("k").unwrap();
        assert!(matches!(store.get("k"), Err(SecretError::NotFound { .. })));
    }

    #[test]
    fn deleting_an_absent_secret_is_not_an_error() {
        let store = InMemorySecretStore::new();
        store.delete("never-existed").unwrap();
    }

    #[test]
    fn provider_keys_are_namespaced() {
        assert_eq!(provider_key("anthropic"), "provider.anthropic.api_key");
    }

    #[test]
    fn environment_variable_names_cover_both_conventions() {
        let names = EnvSecretStore::variables_for(&provider_key("anthropic"));
        assert_eq!(
            names,
            vec![
                "AGENTOS_PROVIDER_ANTHROPIC_API_KEY".to_owned(),
                "ANTHROPIC_API_KEY".to_owned(),
            ]
        );

        // A non-provider key gets only the namespaced form.
        assert_eq!(
            EnvSecretStore::variables_for("browser.token"),
            vec!["AGENTOS_BROWSER_TOKEN".to_owned()]
        );
    }

    #[test]
    fn the_environment_store_is_read_only() {
        let store = EnvSecretStore::new();
        assert!(!store.is_writable());
        assert!(matches!(
            store.set("provider.x.api_key", "v"),
            Err(SecretError::ReadOnly { .. })
        ));
        assert!(matches!(
            store.delete("provider.x.api_key"),
            Err(SecretError::ReadOnly { .. })
        ));
    }

    #[test]
    fn a_chain_reads_through_and_writes_to_the_first_writable_store() {
        let writable = Arc::new(InMemorySecretStore::new());
        let readonly = Arc::new(EnvSecretStore::new());
        let chain = ChainSecretStore::new(vec![readonly, writable.clone()]);

        assert!(chain.is_writable());
        chain.set("k", "v").unwrap();
        // The write skipped the read-only store and landed in the writable one.
        assert_eq!(writable.get("k").unwrap().expose(), "v");
        assert_eq!(chain.get("k").unwrap().expose(), "v");
        assert_eq!(chain.locate("k").map(|(name, _)| name), Some("in-memory"));

        chain.delete("k").unwrap();
        assert!(matches!(chain.get("k"), Err(SecretError::NotFound { .. })));
        assert!(chain.locate("k").is_none());
    }

    #[test]
    fn earlier_stores_in_a_chain_win() {
        let first = Arc::new(InMemorySecretStore::new());
        let second = Arc::new(InMemorySecretStore::new());
        first.set("k", "from-first").unwrap();
        second.set("k", "from-second").unwrap();

        let chain = ChainSecretStore::new(vec![first, second]);
        assert_eq!(chain.get("k").unwrap().expose(), "from-first");
    }

    #[test]
    fn an_unavailable_store_does_not_mask_a_missing_credential() {
        // On a machine with no keychain, every lookup would otherwise report
        // "no default store has been set" rather than "no key is configured",
        // which is the wrong error in the overwhelmingly common case.
        #[derive(Debug)]
        struct BrokenStore;

        impl SecretStore for BrokenStore {
            fn name(&self) -> &'static str {
                "broken"
            }

            fn get(&self, key: &str) -> Result<Secret, SecretError> {
                Err(SecretError::Backend {
                    key: key.to_owned(),
                    message: "no default store has been set".to_owned(),
                })
            }

            fn set(&self, _key: &str, _value: &str) -> Result<(), SecretError> {
                Ok(())
            }

            fn delete(&self, _key: &str) -> Result<(), SecretError> {
                Ok(())
            }
        }

        let chain =
            ChainSecretStore::new(vec![Arc::new(BrokenStore), Arc::new(EnvSecretStore::new())]);
        assert!(
            matches!(
                chain.get("provider.nothing.api_key"),
                Err(SecretError::NotFound { .. })
            ),
            "a broken store must not masquerade as a missing-credential error"
        );

        // And a working store later in the chain still answers.
        let working = Arc::new(InMemorySecretStore::new());
        working.set("k", "v").unwrap();
        let chain = ChainSecretStore::new(vec![Arc::new(BrokenStore), working]);
        assert_eq!(chain.get("k").unwrap().expose(), "v");
    }

    #[test]
    fn a_chain_with_no_writable_store_says_so() {
        let chain = ChainSecretStore::new(vec![Arc::new(EnvSecretStore::new())]);
        assert!(!chain.is_writable());
        assert!(matches!(
            chain.set("k", "v"),
            Err(SecretError::ReadOnly { .. })
        ));
    }

    #[test]
    fn probing_the_keychain_never_panics() {
        // On a developer machine this is `Available`; in CI it is usually not.
        // Either is a valid answer — what must not happen is a crash or a hang.
        let status = KeyringStore::status();
        match &status {
            KeychainStatus::Available => assert!(status.is_available()),
            KeychainStatus::Unavailable { reason } => {
                assert!(!reason.is_empty(), "an unavailable keychain must say why");
                assert!(!status.is_available());
            }
        }
    }
}
