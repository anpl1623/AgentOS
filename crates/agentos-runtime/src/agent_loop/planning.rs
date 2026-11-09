//! The planning state: asking the model what to do next, within the run's
//! own step limit and the step budget of the tree it belongs to, retrying a
//! provider that fails in a way worth retrying.

use agentos_core::event::AgentEvent;
use agentos_core::task::{TaskFailure, TaskStepKind, TaskTrigger};
use agentos_core::trust::{Message, Role};
use agentos_providers::{CompletionRequest, CompletionResponse, ProviderError};

use super::{AgentLoop, PROVIDER_RETRIES, PROVIDER_RETRY_DELAY};
use crate::error::RuntimeError;
use crate::prompt::system_prompt;

impl AgentLoop {
    /// Ask the model what to do next.
    ///
    /// Two limits apply. The agent's own `max_steps` bounds this run, as the
    /// operator set it for this agent; the budget bounds the whole delegation
    /// tree the run belongs to. For a run that delegates to nothing the budget
    /// was granted exactly `max_steps` and only this run spends it, so the two
    /// agree turn for turn and the run stops where it always did.
    pub(super) async fn plan(&mut self) -> Result<TaskTrigger, RuntimeError> {
        if self.run.steps_taken >= self.agent.max_steps {
            self.pending_failure = Some(TaskFailure::StepBudgetExhausted {
                limit: self.agent.max_steps,
            });
            return Ok(TaskTrigger::UnrecoverableError);
        }
        // Taken before the provider is called, so that two runs of one tree
        // cannot both be promised its last turn, and given back below if the
        // call produces no turn.
        if !self.budget.try_spend() {
            self.pending_failure = Some(TaskFailure::StepBudgetExhausted {
                limit: self.budget.granted(),
            });
            return Ok(TaskTrigger::UnrecoverableError);
        }

        let tools = self
            .pipeline
            .registry()
            .metadata_for(&self.agent.enabled_tools);

        let request = CompletionRequest::new(
            &self.agent.model.model,
            system_prompt(&self.agent.instructions),
        )
        .with_messages(self.conversation.clone())
        .with_tools(tools.clone())
        .with_max_output_tokens(self.agent.model.max_output_tokens)
        .with_temperature(self.agent.model.temperature);

        self.machine
            .emit(AgentEvent::ModelRequestStarted {
                provider: self.provider.id().to_owned(),
                model: self.agent.model.model.clone(),
                message_count: self.conversation.len(),
                tool_count: tools.len(),
            })
            .await;

        let started = std::time::Instant::now();
        let response = match self.call_provider(request).await {
            Ok(response) => response,
            Err(error) => {
                // Not a turn: the run record does not count it, so the tree's
                // budget must not either.
                self.budget.refund();
                self.machine
                    .emit(AgentEvent::ModelRequestFailed {
                        provider: self.provider.id().to_owned(),
                        error: error.to_string(),
                    })
                    .await;

                if matches!(error, ProviderError::Cancelled) {
                    return Ok(TaskTrigger::Cancel);
                }

                self.pending_failure = Some(TaskFailure::Provider {
                    message: error.to_string(),
                });
                return Ok(
                    if error.is_retryable() && self.recovery_attempts < PROVIDER_RETRIES {
                        TaskTrigger::RecoverableError
                    } else {
                        TaskTrigger::UnrecoverableError
                    },
                );
            }
        };

        self.run.steps_taken += 1;
        self.run.input_tokens += response.usage.input_tokens.unwrap_or(0);
        self.run.output_tokens += response.usage.output_tokens.unwrap_or(0);
        self.database.runs().update(&self.run).await?;

        self.machine
            .emit(AgentEvent::ModelRequestCompleted {
                provider: self.provider.id().to_owned(),
                model: self.agent.model.model.clone(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                tool_calls: response.tool_calls().len(),
            })
            .await;

        let text = response.text();
        self.machine
            .emit(AgentEvent::ReasoningCompleted {
                summary: truncate(&text, 400),
                tool_calls: response.tool_calls().len(),
            })
            .await;

        self.record_step(
            TaskStepKind::Planning,
            if text.is_empty() {
                format!("Requested {} tool call(s)", response.tool_calls().len())
            } else {
                truncate(&text, 200)
            },
            None,
            Some(serde_json::json!({
                "text": text,
                "tool_calls": response.tool_calls().len(),
            })),
        )
        .await?;

        // The model's own turn goes back into the conversation as model output,
        // never as control-plane content.
        self.conversation
            .push(Message::new(Role::Assistant, response.content.clone()));

        let wants_tools = response.wants_tools();
        self.last_response = Some(response);
        self.recovery_attempts = 0;

        Ok(if wants_tools {
            TaskTrigger::PlanProducedToolCalls
        } else {
            TaskTrigger::PlanProducedNoToolCalls
        })
    }

    async fn call_provider(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let mut attempt = 0;
        loop {
            match self
                .provider
                .complete(request.clone(), self.cancel.clone())
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if error.is_retryable() && attempt < PROVIDER_RETRIES => {
                    attempt += 1;
                    tracing::warn!(%error, attempt, "retrying provider request");
                    tokio::select! {
                        () = self.cancel.cancelled() => return Err(ProviderError::Cancelled),
                        () = tokio::time::sleep(PROVIDER_RETRY_DELAY * attempt) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut cut = max;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}
