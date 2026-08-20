//! Model providers.
//!
//! AgentOS is not built around one vendor. The runtime speaks
//! [`ModelProvider`], and Anthropic, OpenAI-compatible endpoints (which covers
//! OpenAI itself, Ollama, LM Studio and vLLM) and a deterministic mock all
//! implement it. Swapping providers is a configuration change, not a code change.
//!
//! Credentials come from the OS keychain via `agentos-secrets` and are never
//! logged, never persisted to the database, and never included in error text.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod anthropic;
pub mod mock;
pub mod openai;
pub mod request;

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

pub use anthropic::AnthropicProvider;
pub use mock::{MockProvider, ScriptedTurn};
pub use openai::OpenAiCompatibleProvider;
pub use request::{
    CompletionRequest, CompletionResponse, StopReason, Usage, message_for_tool_result,
};

/// Provider identifiers the runtime knows how to construct.
pub mod provider_ids {
    /// Anthropic's Messages API.
    pub const ANTHROPIC: &str = "anthropic";
    /// Any OpenAI-compatible chat-completions endpoint.
    pub const OPENAI: &str = "openai";
    /// A local Ollama server, via its OpenAI-compatible endpoint.
    pub const OLLAMA: &str = "ollama";
    /// The deterministic in-process provider used by tests.
    pub const MOCK: &str = "mock";

    /// Every identifier, for validation and `--help` text.
    pub const ALL: &[&str] = &[ANTHROPIC, OPENAI, OLLAMA, MOCK];
}

/// Something went wrong talking to a model.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// No credential is configured.
    #[error("no API key configured for `{provider}`; run `agentos provider set-key {provider}`")]
    MissingCredential {
        /// The provider.
        provider: String,
    },

    /// The provider rejected the credential.
    #[error("`{provider}` rejected the credential")]
    Unauthorised {
        /// The provider.
        provider: String,
    },

    /// The provider asked us to slow down.
    #[error("`{provider}` rate limited the request{}", retry_after_secs.map(|s| format!("; retry after {s}s")).unwrap_or_default())]
    RateLimited {
        /// The provider.
        provider: String,
        /// Seconds to wait, when reported.
        retry_after_secs: Option<u64>,
    },

    /// The request never completed.
    #[error("`{provider}` request failed: {message}")]
    Transport {
        /// The provider.
        provider: String,
        /// Detail.
        message: String,
    },

    /// The provider returned an error status.
    #[error("`{provider}` returned {status}: {message}")]
    Api {
        /// The provider.
        provider: String,
        /// HTTP status.
        status: u16,
        /// Detail, with credentials stripped.
        message: String,
    },

    /// The response did not look like what the provider documents.
    #[error("`{provider}` returned an unexpected response: {message}")]
    Malformed {
        /// The provider.
        provider: String,
        /// Detail.
        message: String,
    },

    /// The run was cancelled mid-request.
    #[error("cancelled")]
    Cancelled,

    /// The provider identifier is not one we can construct.
    #[error("unknown provider `{0}`")]
    Unknown(String),
}

impl ProviderError {
    /// Whether retrying the same request could succeed.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Transport { .. } => true,
            Self::Api { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

/// What a provider supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Whether it can call tools.
    ///
    /// A provider that cannot is unusable for agent work, so the runtime checks
    /// this rather than discovering it halfway through a task.
    pub tools: bool,
    /// Whether it reports token usage.
    pub usage_reporting: bool,
}

/// Something that can answer a [`CompletionRequest`].
#[async_trait]
pub trait ModelProvider: Send + Sync + fmt::Debug {
    /// Stable identifier, e.g. `anthropic`.
    fn id(&self) -> &str;

    /// What this provider supports.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Ask the model.
    ///
    /// Must return [`ProviderError::Cancelled`] promptly when `cancel` fires.
    ///
    /// # Errors
    ///
    /// Any [`ProviderError`].
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, ProviderError>;
}

/// A provider behind an [`Arc`].
pub type SharedProvider = Arc<dyn ModelProvider>;

/// Redact anything credential-shaped from provider error text.
///
/// Providers echo request context in error bodies, and an operator pasting a
/// stack trace into an issue should not be pasting their key with it.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
        let trimmed = token.trim_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'');
        if looks_like_a_credential(trimmed) {
            out.push_str("[redacted]");
            if let Some(last) = token.chars().last()
                && (last.is_whitespace() || last == '"' || last == '\'')
            {
                out.push(last);
            }
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_like_a_credential(token: &str) -> bool {
    const PREFIXES: &[&str] = &["sk-", "sk_", "pk-", "Bearer", "xoxb-", "ghp_", "gsk_"];
    if token.len() >= 20 && PREFIXES.iter().any(|prefix| token.starts_with(prefix)) {
        return true;
    }
    // A long unbroken run of key-ish characters is almost certainly a secret.
    token.len() >= 32
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && token.chars().any(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_redacted_from_error_text() {
        let text = "request failed with key sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFF and model x";
        let redacted = redact(text);
        assert!(!redacted.contains("AAAABBBB"));
        assert!(redacted.contains("[redacted]"));
        assert!(redacted.contains("and model x"));
    }

    #[test]
    fn ordinary_words_survive_redaction() {
        let text = "the model returned an unexpected shape for messages";
        assert_eq!(redact(text), text);
    }

    #[test]
    fn quoted_keys_are_redacted() {
        let redacted = redact("{\"api_key\": \"sk-proj-0123456789abcdefghijklmn\"}");
        assert!(!redacted.contains("0123456789"));
    }

    #[test]
    fn retryable_errors_are_classified() {
        assert!(
            ProviderError::RateLimited {
                provider: "p".into(),
                retry_after_secs: None
            }
            .is_retryable()
        );
        assert!(
            ProviderError::Api {
                provider: "p".into(),
                status: 503,
                message: String::new()
            }
            .is_retryable()
        );
        assert!(
            !ProviderError::Api {
                provider: "p".into(),
                status: 400,
                message: String::new()
            }
            .is_retryable()
        );
        assert!(
            !ProviderError::Unauthorised {
                provider: "p".into()
            }
            .is_retryable()
        );
        assert!(!ProviderError::Cancelled.is_retryable());
    }
}
