//! Credentials: a model provider's key, and the secrets a run may spend at
//! one origin. What is recorded, and what is listed, says where a secret is
//! bound and under what name, and never the secret.

use agentos_core::event::{AgentEvent, Event};
use agentos_providers::provider_ids;
use agentos_secrets::provider_key;

use crate::credentials::{credential_address, index_entry, index_key, listed};
use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Store a credential for a model provider in the runtime's secret store.
    ///
    /// The record names the provider and nothing else.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know or
    /// an empty key, [`RuntimeError::Secrets`] if the store refuses — on a
    /// machine with no keychain it is read-only — and [`RuntimeError::Audit`]
    /// if the change could not be recorded.
    pub async fn set_provider_key(&self, provider: &str, key: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(RuntimeError::Rejected("no key was provided".to_owned()));
        }
        self.secrets.set(&provider_key(provider), key)?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeySet {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    /// Remove a stored provider credential.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know,
    /// [`RuntimeError::Secrets`] if the store fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remove_provider_key(&self, provider: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        self.secrets.delete(&provider_key(provider))?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeyRemoved {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    /// Store a secret bound to one origin, under a name a request can ask for
    /// it by. Returns the origin in the canonical spelling it is bound under,
    /// which is the spelling a run's request is matched against.
    ///
    /// The record names the origin and the credential and nothing else.
    /// Storing under a name already used for that origin replaces the value.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an origin that is not
    /// `scheme://host[:port]` over http or https, a name that is not a
    /// credential name, or a secret that is empty or holds a control character;
    /// [`RuntimeError::Secrets`] if the store refuses — on a machine with no
    /// keychain it is read-only, and network credentials are not read from the
    /// environment; [`RuntimeError::Database`] if the listing cannot be written;
    /// and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_network_credential(
        &self,
        origin: &str,
        name: &str,
        secret: &str,
    ) -> Result<String, RuntimeError> {
        let (origin, name, key) = credential_address(origin, name)?;
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(RuntimeError::Rejected("no secret was provided".to_owned()));
        }
        // A credential is sent as a header value, where a line break would
        // start a header of the caller's choosing. Refused here, where the
        // operator can be told, rather than at the moment a run sends it.
        if secret.chars().any(char::is_control) {
            return Err(RuntimeError::Rejected(
                "a credential cannot contain line breaks or other control characters".to_owned(),
            ));
        }

        self.secrets.set(&key, secret)?;
        self.database
            .settings()
            .set(&index_key(&key), &index_entry(&origin, &name))
            .await?;
        self.audit
            .record(Event::new(AgentEvent::CredentialSet {
                origin: origin.clone(),
                name,
            }))
            .await?;
        Ok(origin)
    }

    /// Remove a stored network credential.
    ///
    /// Recorded even when nothing was stored under that origin and name: the
    /// record is of what the operator did.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an origin or name that could never have
    /// been stored, [`RuntimeError::Secrets`] if the store fails,
    /// [`RuntimeError::Database`] if the listing cannot be updated, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remove_network_credential(
        &self,
        origin: &str,
        name: &str,
    ) -> Result<(), RuntimeError> {
        let (origin, name, key) = credential_address(origin, name)?;
        // The value first: if it cannot be removed, the listing goes on
        // saying it is there, which is true.
        self.secrets.delete(&key)?;
        self.database.settings().delete(&index_key(&key)).await?;
        self.audit
            .record(Event::new(AgentEvent::CredentialRemoved { origin, name }))
            .await?;
        Ok(())
    }

    /// Every stored network credential as `(origin, name)`, by origin and then
    /// name. Never a value, nor anything derived from one.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn list_network_credentials(&self) -> Result<Vec<(String, String)>, RuntimeError> {
        let mut listed: Vec<(String, String)> = self
            .database
            .settings()
            .all()
            .await?
            .iter()
            .filter_map(listed)
            .collect();
        listed.sort();
        Ok(listed)
    }
}

/// Refuse a provider identifier the runtime cannot build.
///
/// A credential stored under a misspelt provider would never be read, and its
/// record would name a provider that does not exist.
fn known_provider(provider: &str) -> Result<(), RuntimeError> {
    if provider_ids::ALL.contains(&provider) {
        Ok(())
    } else {
        Err(RuntimeError::Rejected(format!(
            "unknown provider `{provider}`; expected one of {}",
            provider_ids::ALL.join(", ")
        )))
    }
}
