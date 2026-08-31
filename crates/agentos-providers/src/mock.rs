//! A deterministic in-process provider.
//!
//! The whole test suite runs against this: no network, no API key, no cost, no
//! flake. A scripted provider also makes it possible to test things a real model
//! makes hard to reproduce on demand — in particular, an agent that has been
//! fully hijacked by injected text and is now issuing an attacker's tool calls.
//! That scenario is a fixture here rather than a hope.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use agentos_core::tool::ToolCall;
use agentos_core::trust::{Content, Message};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::request::{CompletionRequest, CompletionResponse, StopReason, Usage};
use crate::{ModelProvider, ProviderCapabilities, ProviderError};

/// One scripted model turn.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedTurn {
    /// Reply with prose and finish.
    Text(String),
    /// Reply with prose and ask for tools.
    ToolCalls {
        /// Accompanying prose.
        text: String,
        /// The calls to request.
        calls: Vec<ToolCall>,
    },
    /// Fail, to exercise recovery.
    Error(String),
}

impl ScriptedTurn {
    /// A turn that finishes with a final answer.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// A turn that requests a single tool call.
    #[must_use]
    pub fn call(id: &str, tool: &str, arguments: serde_json::Value) -> Self {
        Self::ToolCalls {
            text: format!("Calling {tool}."),
            calls: vec![ToolCall::new(id, tool, arguments)],
        }
    }
}

/// A provider that replays a fixed script.
#[derive(Debug)]
pub struct MockProvider {
    script: Mutex<Vec<ScriptedTurn>>,
    cursor: AtomicUsize,
    seen: Mutex<Vec<CompletionRequest>>,
    /// What to do once the script runs out.
    exhausted: ScriptedTurn,
    /// Whether this mock claims to accept images.
    ///
    /// Off by default so the suite's baseline is the harder case: a model that
    /// cannot see, which is the one where a screenshot has to degrade honestly
    /// rather than vanish.
    vision: bool,
}

impl MockProvider {
    /// Build a provider from a script.
    ///
    /// Once the script is exhausted the provider returns a final text turn, so a
    /// runaway loop ends rather than hanging a test.
    #[must_use]
    pub fn new(script: Vec<ScriptedTurn>) -> Self {
        Self {
            script: Mutex::new(script),
            cursor: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
            exhausted: ScriptedTurn::Text("Done.".to_owned()),
            vision: false,
        }
    }

    /// A provider that immediately answers with one line of text.
    #[must_use]
    pub fn answering(text: impl Into<String>) -> Self {
        Self::new(vec![ScriptedTurn::text(text)])
    }

    /// Change what happens after the script is exhausted.
    #[must_use]
    pub fn with_exhausted(mut self, turn: ScriptedTurn) -> Self {
        self.exhausted = turn;
        self
    }

    /// Every request this provider has been given.
    ///
    /// Lets a test assert on what the runtime actually sent — for example that
    /// tool output arrived wrapped in an untrusted-data envelope.
    #[must_use]
    pub fn requests(&self) -> Vec<CompletionRequest> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    /// How many turns have been consumed.
    #[must_use]
    pub fn turns_taken(&self) -> usize {
        self.cursor.load(Ordering::SeqCst)
    }

    /// The rendered text of every message in the most recent request.
    ///
    /// Declare that this mock accepts images.
    #[must_use]
    pub const fn seeing(mut self) -> Self {
        self.vision = true;
        self
    }

    /// Every image the model was shown in the most recent request.
    #[must_use]
    pub fn last_images(&self) -> Vec<agentos_core::trust::UntrustedImage> {
        self.requests()
            .last()
            .map(|request| {
                request
                    .messages
                    .iter()
                    .flat_map(|message: &Message| message.content.iter())
                    .filter_map(Content::image)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Useful for asserting on what the model was actually shown.
    #[must_use]
    pub fn last_rendered_conversation(&self) -> String {
        self.requests()
            .last()
            .map(|request| {
                request
                    .messages
                    .iter()
                    .flat_map(|message: &Message| message.content.iter())
                    .map(Content::render)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn id(&self) -> &str {
        crate::provider_ids::MOCK
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            tools: true,
            usage_reporting: true,
            vision: self.vision,
        }
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }

        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request);
        }

        let index = self.cursor.fetch_add(1, Ordering::SeqCst);
        let turn = self
            .script
            .lock()
            .ok()
            .and_then(|script| script.get(index).cloned())
            .unwrap_or_else(|| self.exhausted.clone());

        match turn {
            ScriptedTurn::Error(message) => Err(ProviderError::Api {
                provider: crate::provider_ids::MOCK.to_owned(),
                status: 500,
                message,
            }),
            ScriptedTurn::Text(text) => Ok(CompletionResponse {
                content: vec![Content::Model(text)],
                stop_reason: StopReason::EndTurn,
                usage: Usage {
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                },
            }),
            ScriptedTurn::ToolCalls { text, calls } => {
                let mut content = vec![Content::Model(text)];
                content.extend(calls.into_iter().map(Content::ToolCall));
                Ok(CompletionResponse {
                    content,
                    stop_reason: StopReason::ToolUse,
                    usage: Usage {
                        input_tokens: Some(0),
                        output_tokens: Some(0),
                    },
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CompletionRequest {
        CompletionRequest::new("scripted", "be helpful")
            .with_messages(vec![Message::objective("do the thing")])
    }

    #[tokio::test]
    async fn replays_the_script_in_order() {
        let provider = MockProvider::new(vec![
            ScriptedTurn::call("c1", "filesystem.read", serde_json::json!({"path": "a"})),
            ScriptedTurn::text("All done."),
        ]);

        let first = provider
            .complete(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        assert_eq!(first.tool_calls().len(), 1);

        let second = provider
            .complete(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        assert_eq!(second.text(), "All done.");
        assert_eq!(provider.turns_taken(), 2);
    }

    #[tokio::test]
    async fn an_exhausted_script_terminates_rather_than_looping() {
        let provider = MockProvider::new(vec![]);
        let response = provider
            .complete(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[tokio::test]
    async fn scripted_errors_surface_as_provider_errors() {
        let provider = MockProvider::new(vec![ScriptedTurn::Error("overloaded".into())]);
        let error = provider
            .complete(request(), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(error.is_retryable());
    }

    #[tokio::test]
    async fn cancellation_is_honoured() {
        let provider = MockProvider::answering("hi");
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            provider.complete(request(), cancel).await,
            Err(ProviderError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn requests_are_recorded_for_inspection() {
        let provider = MockProvider::answering("hi");
        provider
            .complete(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(provider.requests().len(), 1);
        assert!(
            provider
                .last_rendered_conversation()
                .contains("do the thing")
        );
    }
}
