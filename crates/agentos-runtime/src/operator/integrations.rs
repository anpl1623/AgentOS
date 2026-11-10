//! Integration accounts: binding one, unbinding it, and listing what is bound.
//!
//! An account is a credential and the row that points at it. Both halves are
//! written, removed and recorded here, each as itself, in the order that
//! leaves a secret nothing points at rather than an account with nothing
//! behind it.

use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::IntegrationAccountId;
use agentos_persistence::integrations::IntegrationAccount;
use agentos_tools::{CredentialResolver, MIN_REDACTED_FRAGMENT, Secret};

use crate::credentials::SecretStoreResolver;
use crate::integrations::{BindingTarget, BoundAccount, account_of, binding_target};
use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Check a binding as [`Self::bind_integration`] would, before a token is
    /// asked for: the integration, label, host, flag and note as
    /// [`binding_target`] checks them, and then that nothing is stored where
    /// the token would go.
    ///
    /// Two things can already be there. An account bound under the same label
    /// is using the credential the token would replace. A network credential
    /// stored for the same origin under the same name, with no account
    /// behind it, is one the operator stored for `network.request`, and
    /// binding over it would replace it silently and then, on unbinding,
    /// delete it. Both are refused, and the refusal says which.
    ///
    /// # Errors
    ///
    /// As [`binding_target`], and [`RuntimeError::Rejected`] for either
    /// conflict above; [`RuntimeError::Database`] if the accounts cannot be
    /// read.
    pub async fn check_binding(
        &self,
        integration: &str,
        label: &str,
        host: Option<&str>,
        private_network: bool,
        scopes: Option<&str>,
    ) -> Result<BindingTarget, RuntimeError> {
        let target = binding_target(integration, label, host, private_network, scopes)?;
        if self
            .database
            .integrations()
            .find(integration, label)
            .await?
            .is_some()
        {
            return Err(RuntimeError::Rejected(format!(
                "a {integration} account labelled `{label}` is already bound; remove it first"
            )));
        }
        let backed = self
            .list_integrations()
            .await?
            .iter()
            .any(|bound| bound.uses_credential(&target.origin, label));
        let stored = SecretStoreResolver::new(self.secrets.clone())
            .resolve(&target.origin, label)
            .await
            .is_some();
        if stored && !backed {
            return Err(RuntimeError::Rejected(format!(
                "a network credential named `{label}` is already stored for {}, and no account \
                 uses it; remove it with `agentos credential remove` or under Network \
                 credentials, or bind this account under another label",
                target.origin
            )));
        }
        Ok(target)
    }

    /// Bind an account of an integration: its token, then the row naming it.
    ///
    /// Checked first by [`Self::check_binding`], and then the note is checked
    /// against the token itself, so that a token pasted into both fields is
    /// refused before anything is stored. The token is stored through
    /// [`Self::set_network_credential`], for the origin of the account's host
    /// and under its label, and is recorded there as
    /// `operator.credential.set`. The row is written after it and recorded as
    /// `operator.integration.bound`, so a failure between the two leaves a
    /// secret nothing points at, which `agentos credential list` shows,
    /// rather than an account with nothing behind it.
    ///
    /// `private_network` is the only thing that lets an account's requests
    /// reach a private-network address, and this method is the only writer of
    /// it. The host defaults to the integration's own.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for anything [`Self::check_binding`]
    /// refuses, a note holding part of the token, or an empty token;
    /// [`RuntimeError::Secrets`] if the store refuses;
    /// [`RuntimeError::Database`] if the row cannot be written; and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn bind_integration(
        &self,
        integration: &str,
        label: &str,
        host: Option<&str>,
        private_network: bool,
        scopes: Option<&str>,
        token: Secret,
    ) -> Result<IntegrationAccount, RuntimeError> {
        let target = self
            .check_binding(integration, label, host, private_network, scopes)
            .await?;
        if let Some(scopes) = &target.scopes
            && shows_any_of(scopes, token.expose())
        {
            return Err(RuntimeError::Rejected(
                "the scopes note holds part of the token; the token goes in its own field, and \
                 the note is kept in the database and shown on every listing"
                    .to_owned(),
            ));
        }

        self.set_network_credential(&target.origin, label, token.expose())
            .await?;
        // Zeroed now that the store has it, rather than when the call ends.
        drop(token);

        let account = IntegrationAccount::new(
            integration,
            label,
            &target.host,
            private_network,
            target.scopes,
        );
        self.database.integrations().insert(&account).await?;
        self.audit
            .record(Event::new(AgentEvent::IntegrationBound {
                integration: account.integration.clone(),
                label: account.label.clone(),
                host: account.host.clone(),
                private_network: account.private_network,
            }))
            .await?;
        Ok(account)
    }

    /// Unbind an account: the row, then the token behind it.
    ///
    /// Recorded as `operator.integration.unbound` once the row is gone, which
    /// is the moment no call can act as the account. The token is removed
    /// after, through [`Self::remove_network_credential`], which records its
    /// own `operator.credential.removed`; a crash between the two leaves an
    /// orphan secret rather than a row pointing at nothing. It is kept if
    /// another bound account still names the same origin and label.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if there is no such account or the write
    /// fails, [`RuntimeError::Secrets`] if the token cannot be removed, and
    /// [`RuntimeError::Audit`] if a change could not be recorded. The token is
    /// removed even when the unbinding could not be recorded, and the audit
    /// error is returned after.
    pub async fn unbind_integration(
        &self,
        account_id: IntegrationAccountId,
    ) -> Result<IntegrationAccount, RuntimeError> {
        let account = self.database.integrations().delete(account_id).await?;
        let recorded = self
            .audit
            .record(Event::new(AgentEvent::IntegrationUnbound {
                integration: account.integration.clone(),
                label: account.label.clone(),
                host: account.host.clone(),
                private_network: account.private_network,
            }))
            .await;

        match account_of(&account).origin() {
            Ok(origin) => {
                let shared = self
                    .database
                    .integrations()
                    .list()
                    .await?
                    .iter()
                    .any(|other| {
                        other.label == account.label
                            && account_of(other).origin().is_ok_and(|o| o == origin)
                    });
                if !shared {
                    self.remove_network_credential(&origin, &account.label)
                        .await?;
                }
            }
            // Only a row edited behind the runtime's back has a host that
            // does not read as an origin, and its token, if it has one, is
            // listed by `agentos credential list` for the operator to remove.
            Err(error) => {
                tracing::warn!(%error, "the unbound account's token could not be located");
            }
        }
        recorded?;
        Ok(account)
    }

    /// Every bound account, by integration and then label, with whether a
    /// token is stored behind it.
    ///
    /// Whether one is stored is asked the way a run asks, through the same
    /// resolver, so "present" here means a call would find it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn list_integrations(&self) -> Result<Vec<BoundAccount>, RuntimeError> {
        let resolver = SecretStoreResolver::new(self.secrets.clone());
        let mut bound = Vec::new();
        for account in self.database.integrations().list().await? {
            let origin = account_of(&account).origin().ok();
            let credential_present = match &origin {
                Some(origin) => resolver.resolve(origin, &account.label).await.is_some(),
                None => false,
            };
            bound.push(BoundAccount {
                account,
                origin,
                credential_present,
            });
        }
        bound.sort_by(|a, b| {
            (&a.account.integration, &a.account.label)
                .cmp(&(&b.account.integration, &b.account.label))
        });
        Ok(bound)
    }
}

/// Whether `text` shows `value`, or any run of [`MIN_REDACTED_FRAGMENT`] bytes
/// of it: what the pipeline would redact, had `value` been released.
fn shows_any_of(text: &str, value: &str) -> bool {
    if value.len() < MIN_REDACTED_FRAGMENT {
        return !value.is_empty() && text.contains(value);
    }
    let text = text.as_bytes();
    value
        .as_bytes()
        .windows(MIN_REDACTED_FRAGMENT)
        .any(|run| text.windows(MIN_REDACTED_FRAGMENT).any(|seen| seen == run))
}
