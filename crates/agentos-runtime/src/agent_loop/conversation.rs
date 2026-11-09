//! What the model is shown: the objective and recalled memory to begin
//! with, the run's taint kept in step with what it has read, and old
//! captures dropped so a long run stays inside its image budget.

use agentos_core::memory::MemoryQuery;
use agentos_core::trust::{Content, DataSource, Message};

use super::AgentLoop;
use crate::error::RuntimeError;
use crate::prompt::{PLANNING_MEMORY_KINDS, memory_message};

/// How many captures the conversation keeps before older ones are replaced by
/// their description.
///
/// Three is enough for "before, during, after" and small enough that a long run
/// does not turn into a slideshow the model pays for on every turn.
const MAX_CONVERSATION_IMAGES: usize = 3;

impl AgentLoop {
    /// Put recalled memory and the objective in front of the model.
    ///
    /// A memory recorded from an external source is attacker-controlled text
    /// that has merely been stored for a while. The prompt already wraps it as
    /// untrusted; the tracker has to hear about it too, or a fact the agent
    /// wrote down from a hostile page in one run would let the next run act on
    /// it without a human seeing. Returns the source that flipped the run to
    /// tainted, if one did, so the caller can announce it once the run has
    /// been announced.
    pub(super) async fn seed_conversation(
        &mut self,
        objective: &str,
    ) -> Result<Option<DataSource>, RuntimeError> {
        let memories = self
            .database
            .memories()
            .query(&MemoryQuery::for_agent(self.agent.id).of_kinds(PLANNING_MEMORY_KINDS))
            .await?;

        let mut raised_by = None;
        for memory in memories.iter().filter(|m| m.is_from_untrusted_source()) {
            if self.taint.observe(&memory.source) {
                raised_by = Some(memory.source.clone());
            }
        }

        if let Some(message) = memory_message(&memories) {
            self.conversation.push(message);
        }
        self.conversation.push(Message::objective(objective));
        Ok(raised_by)
    }

    /// Write the tracker's view of the run to the run record when it differs.
    ///
    /// The sources are persisted as well as the flag so that a retry of this
    /// task can start from them. They only ever grow, so a change in either is
    /// a change in length or a change of flag.
    pub(super) async fn sync_taint(&mut self) -> Result<(), RuntimeError> {
        let tainted = self.taint.is_tainted();
        let sources = self.taint.sources();
        if tainted == self.run.tainted && sources.len() == self.run.taint_sources.len() {
            return Ok(());
        }
        self.run.tainted = tainted;
        self.run.taint_sources = sources;
        self.database.runs().update(&self.run).await?;
        Ok(())
    }

    /// Keep only the most recent captures in the conversation.
    ///
    /// Providers re-read the whole conversation on every turn, so an image sent
    /// once is an image paid for on every subsequent request. A run that takes
    /// twelve screenshots and keeps them all is quadratic in cost and will
    /// exhaust a context window long before it exhausts its step budget.
    ///
    /// Older captures are replaced by the description they already carried, so
    /// the model still knows what it looked at and when — it simply cannot look
    /// again without taking a new one.
    pub(super) fn forget_stale_images(&mut self) {
        let total: usize = self
            .conversation
            .iter()
            .map(|message| {
                message
                    .content
                    .iter()
                    .filter(|c| c.image().is_some())
                    .count()
            })
            .sum();
        if total <= MAX_CONVERSATION_IMAGES {
            return;
        }

        let mut to_drop = total - MAX_CONVERSATION_IMAGES;
        for message in &mut self.conversation {
            if to_drop == 0 {
                break;
            }
            for content in &mut message.content {
                if to_drop == 0 {
                    break;
                }
                let Some(image) = content.image() else {
                    continue;
                };
                // Still untrusted, and still from the same source: dropping the
                // pixels does not launder where they came from.
                let replacement = Content::Untrusted(agentos_core::trust::UntrustedContent::new(
                    image.source.clone(),
                    format!(
                        "{} — dropped from the conversation to stay inside the image budget; \
                             take another capture if you need to look again.",
                        image.describe()
                    ),
                ));
                *content = replacement;
                to_drop -= 1;
            }
        }
    }
}
