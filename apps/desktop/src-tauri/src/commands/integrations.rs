//! Integrations: binding the accounts their tools act as.
//!
//! A token crosses this boundary once, inward, as the argument to
//! [`bind_integration`], and is handed to the runtime as a
//! [`Secret`] and dropped. Nothing returns it: binding answers with nothing at
//! all, so the screen reads the list again and shows what the keychain holds
//! rather than what it sent. A refusal is scrubbed of the token before it is
//! shown, as a network credential's is, for the same reason.
//!
//! The token is stored as a network credential for the account's API origin,
//! under the account's label. What an agent may do as the account is decided by
//! its policy: the rules for the integration's domain, and any
//! `network.credential` rule that names that origin and label, which spends
//! the token as `network.request` spends any credential. The token's own scopes
//! widen neither, and nothing here reads them.

use agentos_runtime::Runtime;
use agentos_runtime::integrations::{INTEGRATIONS, binding_target};
use agentos_secrets::{KeychainStatus, KeyringStore};
use agentos_tools::Secret;
use tauri::State;

use super::credentials::without_secret;
use super::{Answer, DesktopError, parse_id};
use crate::dto::{IntegrationTestView, IntegrationView};
use crate::state::AppState;

/// Every integration the runtime ships, with its tools and bound accounts.
#[tauri::command]
pub async fn list_integrations(state: State<'_, AppState>) -> Answer<Vec<IntegrationView>> {
    views(&state.runtime).await
}

/// Bind an account to an integration: its token, then its row.
///
/// Refuses when there is no keychain, as a network credential does and for the
/// same reason: the token is one, and a network credential is never read from
/// the environment, so there is no variable to suggest instead and nowhere
/// weaker this would be willing to put it.
#[tauri::command]
pub async fn bind_integration(
    state: State<'_, AppState>,
    integration: String,
    label: String,
    host: Option<String>,
    private_network: bool,
    scopes: Option<String>,
    token: String,
) -> Answer<()> {
    if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
        return Err(DesktopError::Rejected(format!(
            "this machine has no usable keychain ({}), so there is nowhere secure to store the \
             token. Integration tokens are not read from the environment.",
            reason.lines().next().unwrap_or(&reason)
        )));
    }
    bind(
        &state.runtime,
        &Binding {
            integration: &integration,
            label: &label,
            host: host.as_deref(),
            private_network,
            scopes: scopes.as_deref(),
        },
        token,
    )
    .await
}

/// Unbind an account: its row, then its token.
///
/// Through the runtime, which records the unbinding and the credential's
/// removal, and keeps the token while another bound account still uses it.
#[tauri::command]
pub async fn unbind_integration(state: State<'_, AppState>, account_id: String) -> Answer<()> {
    let id = parse_id("integration account", &account_id)?;
    state.runtime.unbind_integration(id).await?;
    Ok(())
}

/// One authenticated read as an account against its own host.
///
/// Every way the read can fail is an answer rather than an error; only an
/// account that does not exist is refused.
#[tauri::command]
pub async fn test_integration(
    state: State<'_, AppState>,
    account_id: String,
) -> Answer<IntegrationTestView> {
    let id = parse_id("integration account", &account_id)?;
    let check = state.runtime.test_integration(id).await?;
    Ok(IntegrationTestView::from(&check))
}

/// Every shipped integration as a view, with its bound accounts.
///
/// An integration with nothing bound is listed all the same, with its tools:
/// what binding would let an agent use is worth seeing before binding.
pub(crate) async fn views(runtime: &Runtime) -> Answer<Vec<IntegrationView>> {
    let bound = runtime.list_integrations().await?;
    Ok(INTEGRATIONS
        .iter()
        .map(|known| IntegrationView::new(known, &bound))
        .collect())
}

/// What an account is bound as, apart from its token.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Binding<'a> {
    pub integration: &'a str,
    pub label: &'a str,
    pub host: Option<&'a str>,
    pub private_network: bool,
    pub scopes: Option<&'a str>,
}

/// Bind through the runtime.
///
/// The label is trimmed, as a credential's name is, and checked with the
/// host, the private-network flag and the scopes note before the token is
/// looked at, by the runtime's own check, so a refusal for any of them never
/// waits on, or quotes, what was pasted into the token field. The
/// token is trimmed and refused when that leaves nothing, then moved into a
/// [`Secret`]: once the runtime has it, this function holds no copy but the one
/// kept to scrub a refusal, which is dropped on return.
pub(crate) async fn bind(runtime: &Runtime, binding: &Binding<'_>, token: String) -> Answer<()> {
    let label = binding.label.trim();
    binding_target(
        binding.integration,
        label,
        binding.host,
        binding.private_network,
        binding.scopes,
    )?;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(DesktopError::Rejected("no token was provided".to_owned()));
    }
    let secret = Secret::new(trimmed.to_owned());
    match runtime
        .bind_integration(
            binding.integration,
            label,
            binding.host,
            binding.private_network,
            binding.scopes,
            secret,
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => Err(DesktopError::Rejected(without_secret(
            &error.to_string(),
            &token,
        ))),
    }
}
