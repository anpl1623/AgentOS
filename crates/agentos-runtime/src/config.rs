//! Where AgentOS keeps its state, and how providers are constructed.

use std::path::{Path, PathBuf};

use agentos_core::agent::ModelConfig;
use agentos_providers::{
    AnthropicProvider, MockProvider, OpenAiCompatibleProvider, SharedProvider, provider_ids,
};
use agentos_secrets::{SecretError, SecretStore, provider_key};

use crate::error::RuntimeError;

/// Environment variable that relocates the whole AgentOS data directory.
pub const HOME_ENV: &str = "AGENTOS_HOME";

/// Where AgentOS stores everything.
///
/// One directory, on the user's machine. Nothing here is uploaded anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Root data directory, `~/.agentos` unless overridden.
    pub data_dir: PathBuf,
    /// Default agent workspace, where relative paths resolve.
    pub workspace: PathBuf,
    /// SQLite database file.
    pub database_path: PathBuf,
}

impl RuntimeConfig {
    /// Resolve the configuration from the environment.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NoHomeDirectory`] if neither `AGENTOS_HOME` nor a home
    /// directory is available.
    pub fn discover() -> Result<Self, RuntimeError> {
        let data_dir = match std::env::var(HOME_ENV) {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => dirs::home_dir()
                .ok_or(RuntimeError::NoHomeDirectory)?
                .join(".agentos"),
        };
        Ok(Self::rooted_at(data_dir))
    }

    /// Build a configuration rooted at an explicit directory.
    #[must_use]
    pub fn rooted_at(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            workspace: data_dir.join("workspace"),
            database_path: data_dir.join("agentos.db"),
            data_dir,
        }
    }

    /// Create the directories this configuration refers to.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Io`] if a directory cannot be created.
    pub fn ensure_directories(&self) -> Result<(), RuntimeError> {
        for directory in [&self.data_dir, &self.workspace] {
            std::fs::create_dir_all(directory).map_err(|source| {
                RuntimeError::io(format!("creating {}", directory.display()), source)
            })?;
        }
        Ok(())
    }

    /// The workspace directory for one agent.
    #[must_use]
    pub fn workspace_for(&self, agent_name: &str) -> PathBuf {
        self.workspace.join(sanitise_directory_name(agent_name))
    }
}

/// Reduce an agent name to something safe to use as a directory name.
///
/// An agent called `../../etc` must not get a workspace at `/etc`.
fn sanitise_directory_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "agent".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Construct a provider for a model configuration.
///
/// Credentials are read from the secret store at call time and never cached in
/// the database or in the configuration.
///
/// # Errors
///
/// [`RuntimeError::UnknownProvider`] for an unrecognised id, or
/// [`RuntimeError::Provider`] if the client cannot be built. A missing key for a
/// provider that requires one surfaces as
/// [`agentos_providers::ProviderError::MissingCredential`].
pub fn build_provider(
    agent_name: &str,
    model: &ModelConfig,
    secrets: &dyn SecretStore,
) -> Result<SharedProvider, RuntimeError> {
    match model.provider.as_str() {
        provider_ids::ANTHROPIC => {
            let key = required_key(provider_ids::ANTHROPIC, secrets)?;
            Ok(std::sync::Arc::new(AnthropicProvider::new(
                key,
                model.base_url.clone(),
            )?))
        }
        provider_ids::OPENAI => {
            let key = required_key(provider_ids::OPENAI, secrets)?;
            Ok(std::sync::Arc::new(OpenAiCompatibleProvider::new(
                provider_ids::OPENAI,
                Some(key),
                model.base_url.clone(),
            )?))
        }
        // A local server usually wants no credential at all, so a missing key
        // is normal rather than an error.
        provider_ids::OLLAMA => {
            let key = secrets.get(&provider_key(provider_ids::OLLAMA)).ok();
            Ok(std::sync::Arc::new(OpenAiCompatibleProvider::new(
                provider_ids::OLLAMA,
                key,
                model.base_url.clone(),
            )?))
        }
        provider_ids::MOCK => Ok(std::sync::Arc::new(MockProvider::answering(
            "The mock provider does not reason. Configure a real provider with \
             `agentos provider set-key`.",
        ))),
        other => Err(RuntimeError::UnknownProvider {
            agent: agent_name.to_owned(),
            provider: other.to_owned(),
        }),
    }
}

fn required_key(
    provider: &str,
    secrets: &dyn SecretStore,
) -> Result<agentos_secrets::Secret, RuntimeError> {
    match secrets.get(&provider_key(provider)) {
        Ok(secret) if !secret.is_empty() => Ok(secret),
        Ok(_) | Err(SecretError::NotFound { .. }) => Err(RuntimeError::Provider(
            agentos_providers::ProviderError::MissingCredential {
                provider: provider.to_owned(),
            },
        )),
        Err(error) => Err(error.into()),
    }
}

/// Whether a path looks like an AgentOS data directory.
#[must_use]
pub fn is_initialised(data_dir: &Path) -> bool {
    data_dir.join("agentos.db").exists()
}

#[cfg(test)]
mod tests {
    use agentos_secrets::InMemorySecretStore;

    use super::*;

    #[test]
    fn configuration_derives_paths_from_one_root() {
        let config = RuntimeConfig::rooted_at("/data/agentos");
        assert_eq!(config.workspace, Path::new("/data/agentos/workspace"));
        assert_eq!(config.database_path, Path::new("/data/agentos/agentos.db"));
    }

    #[test]
    fn agent_workspaces_cannot_escape_the_workspace_root() {
        let config = RuntimeConfig::rooted_at("/data/agentos");
        let hostile = config.workspace_for("../../etc");
        assert!(hostile.starts_with("/data/agentos/workspace"));
        assert!(!hostile.to_string_lossy().contains(".."));
    }

    #[test]
    fn workspace_names_are_readable_when_they_can_be() {
        let config = RuntimeConfig::rooted_at("/data/agentos");
        assert_eq!(
            config.workspace_for("sales-agent"),
            Path::new("/data/agentos/workspace/sales-agent")
        );
        assert_eq!(
            config.workspace_for("!!!"),
            Path::new("/data/agentos/workspace/agent")
        );
    }

    #[test]
    fn directories_are_created() {
        let dir = tempfile::TempDir::new().unwrap();
        let config = RuntimeConfig::rooted_at(dir.path().join("nested"));
        config.ensure_directories().unwrap();
        assert!(config.workspace.is_dir());
        assert!(!is_initialised(&config.data_dir));
    }

    #[test]
    fn a_provider_without_a_key_reports_it_clearly() {
        let secrets = InMemorySecretStore::new();
        let error = build_provider(
            "sales",
            &ModelConfig::new(provider_ids::ANTHROPIC, "claude-opus-5"),
            &secrets,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no API key configured"));
    }

    #[test]
    fn a_provider_with_a_key_is_constructed() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(&provider_key(provider_ids::ANTHROPIC), "sk-test")
            .unwrap();
        let provider = build_provider(
            "sales",
            &ModelConfig::new(provider_ids::ANTHROPIC, "claude-opus-5"),
            &secrets,
        )
        .unwrap();
        assert_eq!(provider.id(), provider_ids::ANTHROPIC);
        assert!(provider.capabilities().tools);
    }

    #[test]
    fn local_providers_need_no_credential() {
        let secrets = InMemorySecretStore::new();
        let provider = build_provider(
            "local",
            &ModelConfig::new(provider_ids::OLLAMA, "llama3"),
            &secrets,
        )
        .unwrap();
        assert_eq!(provider.id(), provider_ids::OLLAMA);
    }

    #[test]
    fn unknown_providers_are_rejected() {
        let secrets = InMemorySecretStore::new();
        let error = build_provider("x", &ModelConfig::new("wat", "m"), &secrets).unwrap_err();
        assert!(matches!(error, RuntimeError::UnknownProvider { .. }));
    }
}

/// Builds a provider for an agent.
///
/// A trait rather than a bare function so that the runtime has one seam for
/// "where do models come from". Tests substitute a scripted provider through it;
/// a plugin that adds a provider will register through it too.
pub trait ProviderFactory: Send + Sync + std::fmt::Debug {
    /// Build a provider for an agent's model configuration.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] if the provider cannot be constructed.
    fn build(&self, agent_name: &str, model: &ModelConfig) -> Result<SharedProvider, RuntimeError>;
}

/// The production factory: resolves credentials from the secret store.
#[derive(Debug, Clone)]
pub struct SecretBackedProviderFactory {
    secrets: std::sync::Arc<dyn SecretStore>,
}

impl SecretBackedProviderFactory {
    /// Build a factory over a secret store.
    #[must_use]
    pub const fn new(secrets: std::sync::Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }
}

impl ProviderFactory for SecretBackedProviderFactory {
    fn build(&self, agent_name: &str, model: &ModelConfig) -> Result<SharedProvider, RuntimeError> {
        build_provider(agent_name, model, self.secrets.as_ref())
    }
}

/// A factory that always returns the same provider. Tests only.
#[derive(Debug, Clone)]
pub struct FixedProviderFactory {
    provider: SharedProvider,
}

impl FixedProviderFactory {
    /// Wrap a provider.
    #[must_use]
    pub const fn new(provider: SharedProvider) -> Self {
        Self { provider }
    }
}

impl ProviderFactory for FixedProviderFactory {
    fn build(
        &self,
        _agent_name: &str,
        _model: &ModelConfig,
    ) -> Result<SharedProvider, RuntimeError> {
        Ok(self.provider.clone())
    }
}
