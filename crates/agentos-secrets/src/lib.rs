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

impl KeyringStore {
    /// Use the platform keychain under the AgentOS service name.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn entry(key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE_NAME, key).map_err(|error| SecretError::Backend {
            key: key.to_owned(),
            message: error.to_string(),
        })
    }
}

impl SecretStore for KeyringStore {
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

/// The keychain key an LLM provider's credential is stored under.
#[must_use]
pub fn provider_key(provider: &str) -> String {
    format!("provider.{provider}.api_key")
}

#[cfg(test)]
mod tests {
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
}
