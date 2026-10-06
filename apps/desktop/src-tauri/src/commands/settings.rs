//! The tool catalogue, the settings screen and provider credentials.

use agentos_providers::provider_ids;
use agentos_secrets::{
    ChainSecretStore, EnvSecretStore, KeychainStatus, KeyringStore, SecretStore, provider_key,
};
use tauri::State;

use super::{Answer, DesktopError};
use crate::dto::{ProviderView, SettingsView, ToolView};
use crate::state::AppState;

/// Every tool an agent can be granted.
#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Answer<Vec<ToolView>> {
    Ok(state
        .runtime
        .registry()
        .all_metadata()
        .iter()
        .map(ToolView::from)
        .collect())
}

/// The settings screen.
#[tauri::command]
pub async fn settings(state: State<'_, AppState>) -> Answer<SettingsView> {
    let runtime = &state.runtime;
    let config = runtime.config();
    let keychain = KeyringStore::status();
    let secrets = ChainSecretStore::standard();

    let providers = provider_ids::ALL
        .iter()
        .map(|id| {
            let located = secrets.locate(&provider_key(id));
            ProviderView {
                id: (*id).to_owned(),
                configured: located.is_some(),
                hint: located.as_ref().map(|(_, secret)| secret.hint()),
                source: located.as_ref().map(|(store, _)| (*store).to_owned()),
                note: match *id {
                    provider_ids::OLLAMA => "local; usually needs no key".to_owned(),
                    provider_ids::MOCK => "built in; no key needed".to_owned(),
                    _ => EnvSecretStore::variables_for(&provider_key(id))
                        .last()
                        .map(|name| format!("or set {name} in the environment"))
                        .unwrap_or_default(),
                },
            }
        })
        .collect();

    let browser = agentos_browser::locate(None);
    Ok(SettingsView {
        data_dir: config.data_dir.display().to_string(),
        workspace: config.workspace.display().to_string(),
        database: config.database_path.display().to_string(),
        keychain_available: keychain.is_available(),
        keychain_reason: match &keychain {
            KeychainStatus::Available => None,
            KeychainStatus::Unavailable { reason } => {
                Some(reason.lines().next().unwrap_or(reason).to_owned())
            }
        },
        providers,
        browser_path: browser.as_ref().map(|path| path.display().to_string()),
        browser_hint: browser.is_none().then(agentos_browser::install_hint),
        tools: runtime
            .registry()
            .all_metadata()
            .iter()
            .map(ToolView::from)
            .collect(),
    })
}

/// Store a provider credential in the operating system keychain.
///
/// Refuses when there is no keychain rather than pretending to succeed: on such
/// a machine the credential belongs in the environment, and saying so is more
/// use than a generic failure.
#[tauri::command]
pub async fn set_provider_key(provider: String, key: String) -> Answer<()> {
    if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
        let variable = EnvSecretStore::variables_for(&provider_key(&provider))
            .last()
            .cloned()
            .unwrap_or_default();
        return Err(DesktopError::Rejected(format!(
            "this machine has no usable keychain ({}), so there is nowhere secure to store the \
             key. Set {variable} in the environment instead.",
            reason.lines().next().unwrap_or(&reason)
        )));
    }

    let key = key.trim();
    if key.is_empty() {
        return Err(DesktopError::Rejected("no key was provided".to_owned()));
    }
    KeyringStore::new().set(&provider_key(&provider), key)?;
    Ok(())
}

/// Remove a stored provider credential.
#[tauri::command]
pub async fn remove_provider_key(provider: String) -> Answer<()> {
    KeyringStore::new().delete(&provider_key(&provider))?;
    Ok(())
}
