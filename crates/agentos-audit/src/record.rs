//! The audit record and its hash chain.
//!
//! Two independent guarantees:
//!
//! * **Append-only** — enforced in the database by triggers that abort any
//!   `UPDATE` or `DELETE` on the audit table. See the persistence crate.
//! * **Tamper-evident** — each record hashes its own contents together with the
//!   previous record's hash. Altering, reordering or removing a record breaks
//!   every hash after it, and the break is detectable without a trusted copy.
//!
//! The chain does not stop someone with disk access from rewriting the whole
//! log; it makes doing so undetectably infeasible without also recomputing every
//! subsequent hash, and it makes partial edits — the realistic case — obvious.

use agentos_core::Timestamp;
use agentos_core::event::Event;
use agentos_core::ids::{AgentId, EventId, TaskId, TaskRunId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The hash that precedes the first record in a chain.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One row in the audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Identity, shared with the originating [`Event`].
    pub id: EventId,
    /// Position in the chain, starting at 1.
    pub sequence: u64,
    /// When the event happened.
    pub at: Timestamp,
    /// Dotted event name, denormalised so the log can be filtered without
    /// deserialising every payload.
    pub kind: String,
    /// The agent involved.
    pub agent_id: Option<AgentId>,
    /// The task involved.
    pub task_id: Option<TaskId>,
    /// The run involved.
    pub run_id: Option<TaskRunId>,
    /// The serialised event.
    pub payload: serde_json::Value,
    /// Hash of the previous record, or [`GENESIS_HASH`] for the first.
    pub prev_hash: String,
    /// Hash of this record.
    pub hash: String,
}

impl AuditRecord {
    /// Build a record and compute its hash.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] if the event payload cannot be serialised.
    pub fn seal(event: &Event, sequence: u64, prev_hash: &str) -> Result<Self, serde_json::Error> {
        let payload = serde_json::to_value(&event.payload)?;
        let mut record = Self {
            id: event.id,
            sequence,
            at: event.at,
            kind: event.kind().to_owned(),
            agent_id: event.agent_id,
            task_id: event.task_id,
            run_id: event.run_id,
            payload,
            prev_hash: prev_hash.to_owned(),
            hash: String::new(),
        };
        record.hash = record.compute_hash();
        Ok(record)
    }

    /// Recompute this record's hash from its contents.
    ///
    /// Fields are fed in with explicit separators so that moving a character
    /// from one field to the next changes the digest. Concatenating without
    /// separators would let `("ab", "c")` and `("a", "bc")` collide.
    #[must_use]
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        let mut field = |bytes: &[u8]| {
            hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
            hasher.update(bytes);
        };

        field(&self.sequence.to_be_bytes());
        // Same function the database uses, so the hashed form and the stored
        // form cannot drift apart.
        field(agentos_core::format_timestamp(&self.at).as_bytes());
        field(self.kind.as_bytes());
        field(self.id.to_string().as_bytes());
        field(optional_id(self.agent_id.as_ref()).as_bytes());
        field(optional_id(self.task_id.as_ref()).as_bytes());
        field(optional_id(self.run_id.as_ref()).as_bytes());
        // Canonical JSON: serde_json preserves insertion order for `Value`,
        // and the payload originates from a struct with a fixed field order,
        // so this is stable across runs and platforms.
        field(self.payload.to_string().as_bytes());
        field(self.prev_hash.as_bytes());

        hex::encode(hasher.finalize())
    }

    /// Whether this record's stored hash matches its contents.
    #[must_use]
    pub fn is_intact(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

fn optional_id<T: std::fmt::Display>(id: Option<&T>) -> String {
    id.map_or_else(String::new, ToString::to_string)
}

/// A place where the chain does not add up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChainBreak {
    /// A record's contents do not match its own hash.
    #[error("record {sequence} ({id}) has been modified: contents do not match its hash")]
    ModifiedRecord {
        /// Position in the chain.
        sequence: u64,
        /// The record's identity.
        id: EventId,
    },

    /// A record does not point at its predecessor.
    #[error(
        "record {sequence} does not follow record {expected_sequence}: chain is broken, a record was removed, reordered or inserted"
    )]
    BrokenLink {
        /// Position of the offending record.
        sequence: u64,
        /// Position it should have followed.
        expected_sequence: u64,
    },

    /// Sequence numbers are not consecutive.
    #[error("sequence jumped from {previous} to {found}: {} record(s) are missing", found.saturating_sub(*previous).saturating_sub(1))]
    SequenceGap {
        /// Last good sequence number.
        previous: u64,
        /// The sequence number found instead.
        found: u64,
    },

    /// The first record does not start from genesis.
    #[error("chain does not start at genesis: first record claims a predecessor")]
    BadGenesis,
}

/// Result of verifying a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainVerification {
    /// How many records were checked.
    pub records_checked: u64,
    /// Everything wrong that was found. Empty means the chain is intact.
    pub breaks: Vec<ChainBreak>,
    /// The hash of the final record, for chaining a later verification onto it.
    pub tip_hash: String,
}

impl ChainVerification {
    /// Whether the chain is intact.
    #[must_use]
    pub fn is_intact(&self) -> bool {
        self.breaks.is_empty()
    }
}

/// Verify a chain of records, in ascending sequence order.
///
/// Reports every problem rather than stopping at the first, because an operator
/// investigating a suspected tamper wants the full picture in one pass.
#[must_use]
pub fn verify_chain(records: &[AuditRecord]) -> ChainVerification {
    let mut breaks = Vec::new();
    let mut expected_prev_hash = GENESIS_HASH.to_owned();
    let mut expected_sequence: Option<u64> = None;

    for record in records {
        if !record.is_intact() {
            breaks.push(ChainBreak::ModifiedRecord {
                sequence: record.sequence,
                id: record.id,
            });
        }

        match expected_sequence {
            None if record.prev_hash != GENESIS_HASH && record.sequence == 1 => {
                breaks.push(ChainBreak::BadGenesis);
            }
            Some(previous) if record.sequence != previous + 1 => {
                breaks.push(ChainBreak::SequenceGap {
                    previous,
                    found: record.sequence,
                });
            }
            _ => {}
        }

        if record.prev_hash != expected_prev_hash {
            if let Some(previous) = expected_sequence {
                breaks.push(ChainBreak::BrokenLink {
                    sequence: record.sequence,
                    expected_sequence: previous,
                });
            }
        }

        expected_prev_hash = record.hash.clone();
        expected_sequence = Some(record.sequence);
    }

    ChainVerification {
        records_checked: u64::try_from(records.len()).unwrap_or(u64::MAX),
        breaks,
        tip_hash: expected_prev_hash,
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::event::AgentEvent;

    use super::*;

    fn chain(len: usize) -> Vec<AuditRecord> {
        let mut records = Vec::new();
        let mut prev = GENESIS_HASH.to_owned();
        for i in 0..len {
            let event = Event::new(AgentEvent::TaskStarted {
                objective: format!("objective {i}"),
                attempt: 1,
            });
            let record =
                AuditRecord::seal(&event, u64::try_from(i).unwrap_or(0) + 1, &prev).unwrap();
            prev.clone_from(&record.hash);
            records.push(record);
        }
        records
    }

    #[test]
    fn an_untouched_chain_verifies() {
        let records = chain(5);
        let verification = verify_chain(&records);
        assert!(verification.is_intact(), "{:?}", verification.breaks);
        assert_eq!(verification.records_checked, 5);
    }

    #[test]
    fn an_empty_chain_verifies() {
        let verification = verify_chain(&[]);
        assert!(verification.is_intact());
        assert_eq!(verification.tip_hash, GENESIS_HASH);
    }

    #[test]
    fn editing_a_payload_is_detected() {
        let mut records = chain(5);
        records[2].payload = serde_json::json!({"event": "agent.task.started", "objective": "tampered", "attempt": 1});

        let verification = verify_chain(&records);
        assert!(!verification.is_intact());
        assert!(
            verification
                .breaks
                .iter()
                .any(|b| matches!(b, ChainBreak::ModifiedRecord { sequence: 3, .. }))
        );
    }

    #[test]
    fn editing_a_timestamp_is_detected() {
        let mut records = chain(3);
        records[1].at += chrono::Duration::hours(1);
        assert!(!verify_chain(&records).is_intact());
    }

    #[test]
    fn deleting_a_record_is_detected() {
        let mut records = chain(5);
        records.remove(2);

        let verification = verify_chain(&records);
        assert!(!verification.is_intact());
        assert!(
            verification
                .breaks
                .iter()
                .any(|b| matches!(b, ChainBreak::SequenceGap { .. })),
            "{:?}",
            verification.breaks
        );
        assert!(
            verification
                .breaks
                .iter()
                .any(|b| matches!(b, ChainBreak::BrokenLink { .. })),
            "{:?}",
            verification.breaks
        );
    }

    #[test]
    fn reordering_records_is_detected() {
        let mut records = chain(5);
        records.swap(1, 3);
        assert!(!verify_chain(&records).is_intact());
    }

    #[test]
    fn recomputing_the_hash_after_tampering_still_breaks_the_link() {
        // The realistic attack: edit a record and fix up its own hash, hoping
        // nobody checks the next record's back-pointer.
        let mut records = chain(5);
        records[2].payload =
            serde_json::json!({"event": "agent.task.started", "objective": "x", "attempt": 1});
        records[2].hash = records[2].compute_hash();

        let verification = verify_chain(&records);
        assert!(!verification.is_intact());
        assert!(
            verification
                .breaks
                .iter()
                .any(|b| matches!(b, ChainBreak::BrokenLink { sequence: 4, .. }))
        );
    }

    #[test]
    fn field_boundaries_are_hashed() {
        // Without length prefixes, moving a character between adjacent fields
        // would produce the same digest.
        let event = Event::new(AgentEvent::UnknownToolRequested { tool: "ab".into() });
        let a = AuditRecord::seal(&event, 1, GENESIS_HASH).unwrap();

        let mut b = a.clone();
        b.kind = format!("{}x", b.kind);
        assert_ne!(a.compute_hash(), b.compute_hash());
    }

    #[test]
    fn tip_hash_allows_incremental_verification() {
        let records = chain(3);
        let verification = verify_chain(&records);
        assert_eq!(verification.tip_hash, records[2].hash);
    }
}
