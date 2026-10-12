//! The audit chain: its health, and single records opened in full.

use agentos_audit::AuditRecord;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::at;

/// Whether the audit chain still verifies, as of the last check.
///
/// Checked incrementally: each check verifies only the records written since
/// the one before, linked onto the last record already verified. Settings
/// keeps the full rehash as a deliberate act.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AuditHealth {
    /// How many records the log holds.
    #[ts(type = "number")]
    pub events: i64,
    /// Whether every record checked so far verifies and links to the one
    /// before it.
    pub intact: bool,
    /// Audit records this process failed to write since it launched.
    ///
    /// Separate from `intact`, which speaks only for the records the log
    /// holds: a chain can verify perfectly and still be missing what was never
    /// written to it. Anything above zero means the log is incomplete.
    #[ts(type = "number")]
    pub unrecorded: u64,
    /// When this answer was produced.
    pub checked_at: String,
}

/// One audit record, with everything the chain stores for it.
///
/// Unlike [`super::EventView`], nothing is summarised away: the payload is the
/// whole serialised event and both hashes are shown, because this is the view
/// a person opens to check what the log claims.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AuditRecordView {
    /// Identity, shared with the event the record seals.
    pub id: String,
    /// Position in the chain, starting at 1.
    #[ts(type = "number")]
    pub sequence: u64,
    /// When the event happened.
    pub at: String,
    /// The dotted event name.
    pub kind: String,
    /// The agent involved, if any.
    pub agent_id: Option<String>,
    /// The task involved, if any.
    pub task_id: Option<String>,
    /// The run involved, if any.
    pub run_id: Option<String>,
    /// The serialised event, as JSON text laid out for a person to read.
    pub payload: String,
    /// The hash of the record before this one.
    pub prev_hash: String,
    /// This record's own hash.
    pub hash: String,
    /// Whether this records a refusal, an escalation or a rejection.
    pub security_relevant: bool,
}

impl From<&AuditRecord> for AuditRecordView {
    fn from(record: &AuditRecord) -> Self {
        Self {
            id: record.id.to_string(),
            sequence: record.sequence,
            at: at(&record.at),
            kind: record.kind.clone(),
            agent_id: record.agent_id.map(|id| id.to_string()),
            task_id: record.task_id.map(|id| id.to_string()),
            run_id: record.run_id.map(|id| id.to_string()),
            // Pretty-printed like an approval's arguments, falling back to the
            // compact form rather than to nothing.
            payload: serde_json::to_string_pretty(&record.payload)
                .unwrap_or_else(|_| record.payload.to_string()),
            prev_hash: record.prev_hash.clone(),
            hash: record.hash.clone(),
            security_relevant: agentos_core::event::is_security_kind(&record.kind),
        }
    }
}
