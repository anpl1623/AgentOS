//! What an agent remembers, and what reaches its prompt.

use agentos_core::memory::{Memory, MemoryKind};
use agentos_runtime::prompt::PLANNING_MEMORY_KINDS;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::at;

/// One remembered item.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MemoryView {
    /// Identity.
    pub id: String,
    /// The agent that owns it.
    pub agent_id: String,
    /// `fact`, `decision`, `preference`, `task_history` or `observation`.
    pub kind: String,
    /// The content, exactly as stored.
    pub content: String,
    /// Where it came from, labelled the way taint sources are.
    pub source: String,
    /// Whether that source is outside the trust boundary.
    ///
    /// A note derived from a webpage is a claim the webpage made, and the
    /// person reviewing what an agent will be told must be able to tell it
    /// from one they wrote themselves.
    pub source_untrusted: bool,
    /// Whether memories of this kind are retrieved before planning.
    ///
    /// Observations and task history are stored but never shown to the model
    /// up front; an interface implying otherwise would misdescribe what the
    /// model is told.
    pub reaches_the_prompt: bool,
    /// How much to trust it, 0 to 1.
    pub confidence: f32,
    /// The task during which it was recorded.
    pub task_id: Option<String>,
    /// When it was recorded.
    pub created_at: String,
    /// When it was last revised.
    pub updated_at: String,
}

impl From<&Memory> for MemoryView {
    fn from(memory: &Memory) -> Self {
        Self {
            id: memory.id.to_string(),
            agent_id: memory.agent_id.to_string(),
            kind: memory.kind.as_str().to_owned(),
            content: memory.content.clone(),
            source: memory.source.label(),
            source_untrusted: memory.is_from_untrusted_source(),
            reaches_the_prompt: PLANNING_MEMORY_KINDS.contains(&memory.kind),
            confidence: memory.confidence,
            task_id: memory.task_id.map(|id| id.to_string()),
            created_at: at(&memory.created_at),
            updated_at: at(&memory.updated_at),
        }
    }
}

/// What the interface sends to record a memory.
///
/// There is no source field. Whatever arrives is recorded as typed by a
/// person, because that is the only thing this path can attest; see
/// `commands::remember`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RememberInput {
    /// The agent it is for.
    pub agent_id: String,
    /// One of the kinds [`MemoryView::kind`] lists.
    pub kind: String,
    /// What to remember.
    pub content: String,
    /// How much to trust it, 0 to 1. Full confidence when absent.
    pub confidence: Option<f32>,
}

/// Read a memory kind by its wire name.
///
/// `None` for anything else, which callers refuse: a misspelt filter that
/// silently matched nothing would read as an agent with no memories.
#[must_use]
pub fn memory_kind(value: &str) -> Option<MemoryKind> {
    MemoryKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == value)
}
