//! Persistent agent memory.
//!
//! Deliberately simple: structured rows in SQLite with keyword retrieval. The
//! [`MemoryQuery`] shape is what a future semantic index would also satisfy, so
//! swapping the backing store later does not change any call site.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, MemoryId, TaskId};
use crate::trust::DataSource;

/// What kind of thing is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// A stable statement about the world.
    Fact,
    /// A choice that was made and should stay made.
    Decision,
    /// An operator preference.
    Preference,
    /// A summary of past work.
    TaskHistory,
    /// Something noticed during execution.
    Observation,
}

impl MemoryKind {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Preference => "preference",
            Self::TaskHistory => "task_history",
            Self::Observation => "observation",
        }
    }

    /// All kinds.
    pub const ALL: [Self; 5] = [
        Self::Fact,
        Self::Decision,
        Self::Preference,
        Self::TaskHistory,
        Self::Observation,
    ];
}

/// One remembered item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Identity.
    pub id: MemoryId,
    /// The agent that owns it. Memory is not shared between agents implicitly.
    pub agent_id: AgentId,
    /// Kind.
    pub kind: MemoryKind,
    /// The content.
    pub content: String,
    /// Where it came from.
    ///
    /// A memory derived from a webpage is not a fact about the world; it is a
    /// claim a webpage made. Keeping the source means retrieval can say so.
    pub source: DataSource,
    /// How much to trust it, 0.0–1.0.
    pub confidence: f32,
    /// The task during which it was recorded.
    pub task_id: Option<TaskId>,
    /// When recorded.
    pub created_at: Timestamp,
    /// When last revised.
    pub updated_at: Timestamp,
}

impl Memory {
    /// Record a memory.
    #[must_use]
    pub fn new(
        agent_id: AgentId,
        kind: MemoryKind,
        content: impl Into<String>,
        source: DataSource,
    ) -> Self {
        let now = crate::now();
        Self {
            id: MemoryId::new(),
            agent_id,
            kind,
            content: content.into(),
            source,
            confidence: 1.0,
            task_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set confidence, clamped to 0.0–1.0.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Associate with a task.
    #[must_use]
    pub const fn with_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Whether this memory originated outside the trust boundary.
    #[must_use]
    pub const fn is_from_untrusted_source(&self) -> bool {
        self.source.is_externally_influenced()
    }
}

/// A retrieval request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MemoryQuery {
    /// Restrict to one agent.
    pub agent_id: Option<AgentId>,
    /// Restrict to certain kinds.
    pub kinds: Vec<MemoryKind>,
    /// Free text to match. Keyword today, embeddings later.
    pub text: Option<String>,
    /// Drop anything below this confidence.
    pub min_confidence: Option<f32>,
    /// Maximum rows to return.
    pub limit: usize,
}

/// How many memories to retrieve before planning, absent an explicit limit.
pub const DEFAULT_MEMORY_LIMIT: usize = 20;

impl MemoryQuery {
    /// A query scoped to one agent, with the default limit.
    #[must_use]
    pub fn for_agent(agent_id: AgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
            limit: DEFAULT_MEMORY_LIMIT,
            ..Self::default()
        }
    }

    /// Add free-text matching.
    #[must_use]
    pub fn matching(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Restrict to certain kinds.
    #[must_use]
    pub fn of_kinds(mut self, kinds: impl IntoIterator<Item = MemoryKind>) -> Self {
        self.kinds = kinds.into_iter().collect();
        self
    }

    /// Cap the result count.
    #[must_use]
    pub const fn limited_to(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_is_clamped() {
        let memory = Memory::new(AgentId::new(), MemoryKind::Fact, "x", DataSource::User);
        assert_eq!(memory.clone().with_confidence(5.0).confidence, 1.0);
        assert_eq!(memory.with_confidence(-1.0).confidence, 0.0);
    }

    #[test]
    fn web_sourced_memories_are_flagged() {
        let memory = Memory::new(
            AgentId::new(),
            MemoryKind::Observation,
            "the site said X",
            DataSource::Web {
                url: "https://x".into(),
            },
        );
        assert!(memory.is_from_untrusted_source());
    }

    #[test]
    fn queries_default_to_a_bounded_limit() {
        let query = MemoryQuery::for_agent(AgentId::new());
        assert_eq!(query.limit, DEFAULT_MEMORY_LIMIT);
    }
}
