//! Network credentials: secrets bound to one origin each.
//!
//! A secret crosses this boundary once, inward, as the argument to
//! [`set_network_credential`], and is handed to the runtime and dropped. No
//! command returns it, logs it or keeps it, and a refusal is scrubbed of it
//! before it is shown: the window renders errors as they arrive, and an error
//! that quoted what was typed would put the secret on screen in the one place
//! nobody thinks to check. What comes back is where the credential is bound and
//! what it is called.
//!
//! The origin is normalised by the runtime, which keys the secret by it. The
//! answer carries the normalised spelling so the screen can show the operator
//! the origin a run will actually be matched against, not the one they typed.

use agentos_runtime::Runtime;
use agentos_runtime::integrations::BoundAccount;
use agentos_secrets::{KeychainStatus, KeyringStore};
use tauri::State;

use super::{Answer, DesktopError};
use crate::dto::NetworkCredentialView;
use crate::state::AppState;

/// What a secret is replaced with in any text this module passes on.
///
/// The pipeline's marker, so a person who meets it here has met it before.
const REDACTED: &str = "[redacted credential]";

/// Every stored network credential, by origin and name.
#[tauri::command]
pub async fn list_network_credentials(
    state: State<'_, AppState>,
) -> Answer<Vec<NetworkCredentialView>> {
    listed(&state.runtime).await
}

/// Store a credential bound to one origin.
///
/// Refuses when there is no keychain, as provider keys do, though here there is
/// no environment variable to suggest instead: a variable name cannot keep two
/// origins apart, so the runtime reads no network credential from one.
#[tauri::command]
pub async fn set_network_credential(
    state: State<'_, AppState>,
    origin: String,
    name: String,
    secret: String,
) -> Answer<NetworkCredentialView> {
    if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
        return Err(DesktopError::Rejected(format!(
            "this machine has no usable keychain ({}), so there is nowhere secure to store the \
             credential. Network credentials are not read from the environment.",
            reason.lines().next().unwrap_or(&reason)
        )));
    }
    store(&state.runtime, &origin, &name, secret).await
}

/// Remove a stored network credential.
///
/// Through the runtime, which records the removal by origin and name.
#[tauri::command]
pub async fn remove_network_credential(
    state: State<'_, AppState>,
    origin: String,
    name: String,
) -> Answer<()> {
    state
        .runtime
        .remove_network_credential(&origin, &name)
        .await?;
    Ok(())
}

/// The stored credentials as views, in the runtime's order: by origin, then
/// name, each with the account it is the token of, if any.
pub(crate) async fn listed(runtime: &Runtime) -> Answer<Vec<NetworkCredentialView>> {
    let accounts = runtime.list_integrations().await?;
    Ok(runtime
        .list_network_credentials()
        .await?
        .into_iter()
        .map(|(origin, name)| NetworkCredentialView {
            account: account_using(&accounts, &origin, &name),
            origin,
            name,
        })
        .collect())
}

/// The bound account whose token is the credential at `origin` under `name`.
fn account_using(accounts: &[BoundAccount], origin: &str, name: &str) -> Option<String> {
    accounts
        .iter()
        .find(|bound| bound.uses_credential(origin, name))
        .map(BoundAccount::describe)
}

/// Store a credential through the runtime, answering with where it is bound.
///
/// Takes the secret by value so it ends here: once the runtime has it, this
/// function's copy is dropped on return, success or failure.
pub(crate) async fn store(
    runtime: &Runtime,
    origin: &str,
    name: &str,
    secret: String,
) -> Answer<NetworkCredentialView> {
    match runtime.set_network_credential(origin, name, &secret).await {
        // The runtime answers with the origin it normalised; the name it only
        // trims, so trimming here names the credential it stored.
        Ok(origin) => {
            let name = name.trim().to_owned();
            let account = account_using(&runtime.list_integrations().await?, &origin, &name);
            Ok(NetworkCredentialView {
                origin,
                name,
                account,
            })
        }
        Err(error) => Err(DesktopError::Rejected(without_secret(
            &error.to_string(),
            &secret,
        ))),
    }
}

/// `text` with every occurrence of `secret`, as given or trimmed, replaced.
///
/// No refusal the runtime writes today quotes the secret. This is what keeps
/// that true of the ones it writes tomorrow, and of a keychain backend's
/// message, which nobody here controls.
///
/// One pass over the original text rather than a `replace` per spelling: a
/// second pass would search the markers the first one wrote, and a secret
/// that happens to occur in the marker would be replaced inside it. An empty
/// secret matches nothing, where replacing the empty string would insert the
/// marker between every character.
pub(crate) fn without_secret(text: &str, secret: &str) -> String {
    // Longest first, so the whole secret is matched before its trimmed form.
    let spellings: Vec<&str> = [secret, secret.trim()]
        .into_iter()
        .filter(|spelling| !spelling.is_empty())
        .collect();
    let mut scrubbed = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(next) = rest.chars().next() {
        if let Some(spelling) = spellings
            .iter()
            .find(|spelling| rest.starts_with(**spelling))
        {
            scrubbed.push_str(REDACTED);
            rest = &rest[spelling.len()..];
        } else {
            scrubbed.push(next);
            rest = &rest[next.len_utf8()..];
        }
    }
    scrubbed
}
