//! Network credentials: secrets an operator binds to one origin each.
//!
//! The value lives in the secret store under [`network_key`], which spells the
//! origin into the key. A run asks for a credential by name against the origin
//! it is about to reach, and a name asked for at any other origin misses. That
//! is the whole of the binding, and it holds whatever the tool doing the asking
//! believes or has been told.
//!
//! The keychain cannot list what it holds, so each credential also has a row
//! in the settings table naming its origin and name. The row is a reference,
//! as the database is allowed to hold, never the value. It is what the
//! operator layer lists, which means a credential deleted from the keychain
//! behind AgentOS's back is still listed until it is removed through the
//! runtime; a run asking for it meanwhile simply misses.
//!
//! Storing, removing and listing are operator changes, and live with the
//! others in [`crate::operator`]. This module holds what they share with the
//! resolver: how an origin and a name become a key, and how the listing is
//! kept.

use std::sync::Arc;

use agentos_permissions::normalise_origin;
use agentos_secrets::{MAX_CREDENTIAL_NAME_LEN, SecretStore, network_key};
use agentos_tools::{CredentialResolver, Secret};

use crate::RuntimeError;

/// What every credential's row in the settings table is keyed under, before
/// its secret-store key.
const INDEX_PREFIX: &str = "credential.";

/// The runtime's secret store, as a run's tools may consult it.
///
/// Answers only for an origin already in canonical form and a name that is a
/// credential name. Neither check is the binding — the key is — but each
/// refuses a spelling that could only have come from somewhere other than the
/// one place a tool learns an origin, the permissions crate's normaliser.
#[derive(Debug)]
pub struct SecretStoreResolver {
    secrets: Arc<dyn SecretStore>,
}

impl SecretStoreResolver {
    /// Resolve against `secrets`.
    #[must_use]
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }
}

#[async_trait::async_trait]
impl CredentialResolver for SecretStoreResolver {
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret> {
        if normalise_origin(origin).ok()? != origin {
            return None;
        }
        let key = network_key(origin, name)?;
        // A store that cannot answer is a credential the run does not get;
        // the tool reports it as missing and the operator finds out why from
        // `agentos doctor`, which is where keychain health is reported.
        let stored = self.secrets.get(&key).ok()?;
        Some(Secret::new(stored.expose()))
    }
}

/// The canonical origin, the name, and the key they are stored under, or a
/// refusal saying which part cannot be used.
pub(crate) fn credential_address(
    origin: &str,
    name: &str,
) -> Result<(String, String, String), RuntimeError> {
    let origin = normalise_origin(origin.trim())
        .map_err(|error| RuntimeError::Rejected(error.to_string()))?;
    let name = name.trim();
    // The name is not quoted back: a refused name is as likely as not the
    // secret, pasted into the wrong field.
    let key = network_key(&origin, name).ok_or_else(|| {
        RuntimeError::Rejected(format!(
            "a credential name is 1 to {MAX_CREDENTIAL_NAME_LEN} letters, digits, `_` or `-`"
        ))
    })?;
    Ok((origin, name.to_owned(), key))
}

/// The settings row that lists the credential stored under `key`.
pub(crate) fn index_key(key: &str) -> String {
    format!("{INDEX_PREFIX}{key}")
}

/// What a settings row says about a credential, as the row's value.
pub(crate) fn index_entry(origin: &str, name: &str) -> String {
    serde_json::json!({ "origin": origin, "name": name }).to_string()
}

/// The `(origin, name)` a settings row lists, or `None` for a row that is not
/// a credential's.
pub(crate) fn listed((key, entry): &(String, String)) -> Option<(String, String)> {
    if !key.starts_with(INDEX_PREFIX) {
        return None;
    }
    let entry: serde_json::Value = serde_json::from_str(entry).ok()?;
    Some((
        entry.get("origin")?.as_str()?.to_owned(),
        entry.get("name")?.as_str()?.to_owned(),
    ))
}
