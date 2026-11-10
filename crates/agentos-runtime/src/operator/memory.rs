//! Memories, as the operator writes them: remembered, revised and forgotten,
//! each recorded with the text it concerned.

use agentos_core::event::AgentEvent;
use agentos_core::ids::{AgentId, MemoryId};
use agentos_core::memory::{Memory, MemoryKind};
use agentos_core::trust::DataSource;

use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Write a memory for an agent, as the operator.
    ///
    /// The source is always [`DataSource::User`], and no caller can say
    /// otherwise. This method can attest only that a person typed the text. A
    /// caller able to name another source could claim `Web` for a note it
    /// wanted distrusted, or — the direction that matters — launder a webpage's
    /// claim into a trusted note by calling it the operator's. Untrusted
    /// memories exist; they are written by the runtime from what a run read,
    /// never through here.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the agent does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remember(
        &self,
        agent_id: AgentId,
        kind: MemoryKind,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let agent = self.database.agents().get(agent_id).await?;
        let memory =
            Memory::new(agent.id, kind, content, DataSource::User).with_confidence(confidence);
        self.database.memories().insert(&memory).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRemembered {
                agent: agent.name,
                memory_id: memory.id,
                kind: kind.as_str().to_owned(),
                content: memory.content.clone(),
            },
        )
        .await?;
        Ok(memory)
    }

    /// Rewrite a memory's content and confidence.
    ///
    /// Its source is kept. An operator correcting the wording of something a
    /// webpage claimed has not thereby checked the claim, so a revised
    /// untrusted memory stays untrusted. To vouch for a statement, forget the
    /// old memory and remember the statement.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the memory does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn revise_memory(
        &self,
        id: MemoryId,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database
            .memories()
            .update(id, content, confidence)
            .await?;
        let revised = self.database.memories().get(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRevised {
                agent: agent.name,
                memory_id: id,
                content: revised.content.clone(),
            },
        )
        .await?;
        Ok(revised)
    }

    /// Delete a memory.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the memory does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn forget_memory(&self, id: MemoryId) -> Result<(), RuntimeError> {
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database.memories().delete(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryForgotten {
                agent: agent.name,
                memory_id: id,
                kind: memory.kind.as_str().to_owned(),
                content: memory.content,
            },
        )
        .await
    }
}

/// Operator-typed text with its surrounding whitespace removed, or a refusal
/// saying what was missing.
fn non_empty<'a>(text: &'a str, missing: &str) -> Result<&'a str, RuntimeError> {
    let text = text.trim();
    if text.is_empty() {
        Err(RuntimeError::Rejected(missing.to_owned()))
    } else {
        Ok(text)
    }
}
