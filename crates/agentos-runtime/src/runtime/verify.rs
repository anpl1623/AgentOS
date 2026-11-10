//! Proving the audit chain intact, from genesis or from a checkpoint.

use super::Runtime;
use crate::error::RuntimeError;

impl Runtime {
    /// Verify the audit chain, rehashing every record from genesis.
    ///
    /// A log whose oldest records were removed does not verify: the first
    /// record left must be the first record written.
    ///
    /// A delegation tree needs nothing more. The chain is one per
    /// installation, so it already orders every record of every run in a tree
    /// against every other, and each edge between two runs is sealed inside
    /// the hashed payload of its `agent.delegation.started` record. The record
    /// format is unchanged by delegation, deliberately: changing what is
    /// hashed would mean a migration over a log that cannot be rewritten.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn verify_audit(&self) -> Result<agentos_audit::ChainVerification, RuntimeError> {
        let records = self.database.audit_sink().all().await?;
        Ok(agentos_audit::verify_chain(&records))
    }

    /// Verify only the records written since an earlier verification.
    ///
    /// Rehashing the whole chain is the most expensive thing the runtime can
    /// be asked to do, and a screen that polls cannot afford it. This checks
    /// each new record's own hash, that the records follow one another, and
    /// that the first of them points at the checkpoint — so the new part is
    /// proved to extend the part already proved.
    ///
    /// What it cannot see is a change to a record before the checkpoint made
    /// after that record was verified. That is what [`Self::verify_audit`] is
    /// for, and why it stays a deliberate act rather than a poll.
    ///
    /// The checkpoint returned is advanced only past an intact stretch, so a
    /// break goes on being reported until somebody looks at it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn verify_audit_from(
        &self,
        checkpoint: Option<&AuditCheckpoint>,
    ) -> Result<(agentos_audit::ChainVerification, AuditCheckpoint), RuntimeError> {
        let start = checkpoint.cloned().unwrap_or_else(AuditCheckpoint::genesis);
        let records = self.database.audit_sink().after(start.sequence).await?;
        let verification = agentos_audit::verify_chain_from(&records, start.sequence, &start.hash);

        let next = match records.last() {
            Some(last) if verification.is_intact() => AuditCheckpoint {
                sequence: last.sequence,
                hash: last.hash.clone(),
            },
            _ => start,
        };
        Ok((verification, next))
    }
}

/// Where a verification of the audit chain got to.
///
/// Held by a client between calls to [`Runtime::verify_audit_from`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditCheckpoint {
    /// The last record verified; zero before any.
    pub sequence: u64,
    /// Its hash, which the next record must name as its predecessor.
    pub hash: String,
}

impl AuditCheckpoint {
    /// Before the first record.
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            sequence: 0,
            hash: agentos_audit::GENESIS_HASH.to_owned(),
        }
    }
}
