//! The AgentOS audit system: an event bus plus an append-only, hash-chained log.
//!
//! Every meaningful action emits a structured [`Event`]. Events go two places at
//! once: a broadcast channel that live consumers (the desktop activity feed, a
//! CLI trace, a debugging subscriber) read, and a durable sink that writes them
//! to storage in an unforgeable order.
//!
//! Nothing here formats text. Text logs are a rendering of events, produced by
//! whoever is displaying them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod record;

use std::fmt;
use std::sync::Arc;

use agentos_core::event::{AgentEvent, Event};
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::{Mutex, broadcast};

pub use record::{AuditRecord, ChainBreak, ChainVerification, GENESIS_HASH, verify_chain};

/// How many events the live broadcast channel buffers per subscriber.
///
/// A slow subscriber that falls this far behind loses events from its *live*
/// view. It never loses them from the durable log, which is why the durable
/// write is not gated on the broadcast.
pub const BROADCAST_CAPACITY: usize = 1024;

/// Something went wrong recording an event.
#[derive(Debug, Error)]
pub enum AuditError {
    /// The event could not be serialised.
    #[error("cannot serialise event: {0}")]
    Serialisation(#[from] serde_json::Error),

    /// The durable sink failed.
    #[error("audit sink failed: {0}")]
    Sink(String),
}

/// Somewhere audit records are durably written.
///
/// Implementations must be append-only. The SQLite implementation enforces this
/// with database triggers rather than trusting callers.
#[async_trait]
pub trait AuditSink: Send + Sync + fmt::Debug {
    /// Append a record.
    ///
    /// # Errors
    ///
    /// Returns [`AuditError::Sink`] if the write fails.
    async fn append(&self, record: &AuditRecord) -> Result<(), AuditError>;

    /// The sequence number and hash of the last record written, so a restart
    /// continues the existing chain instead of starting a second one.
    ///
    /// # Errors
    ///
    /// Returns [`AuditError::Sink`] if the read fails.
    async fn tip(&self) -> Result<(u64, String), AuditError>;
}

/// A sink that discards everything. For tests that do not assert on the log.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullSink;

#[async_trait]
impl AuditSink for NullSink {
    async fn append(&self, _record: &AuditRecord) -> Result<(), AuditError> {
        Ok(())
    }

    async fn tip(&self) -> Result<(u64, String), AuditError> {
        Ok((0, GENESIS_HASH.to_owned()))
    }
}

/// An in-memory sink that retains everything, for tests that do assert on it.
#[derive(Debug, Default)]
pub struct InMemorySink {
    records: Mutex<Vec<AuditRecord>>,
}

impl InMemorySink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every record written so far.
    pub async fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().await.clone()
    }

    /// Records of one kind.
    pub async fn records_of_kind(&self, kind: &str) -> Vec<AuditRecord> {
        self.records
            .lock()
            .await
            .iter()
            .filter(|record| record.kind == kind)
            .cloned()
            .collect()
    }

    /// Whether any record of the given kind was written.
    pub async fn contains_kind(&self, kind: &str) -> bool {
        self.records
            .lock()
            .await
            .iter()
            .any(|record| record.kind == kind)
    }
}

#[async_trait]
impl AuditSink for InMemorySink {
    async fn append(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.records.lock().await.push(record.clone());
        Ok(())
    }

    async fn tip(&self) -> Result<(u64, String), AuditError> {
        let records = self.records.lock().await;
        Ok(records.last().map_or_else(
            || (0, GENESIS_HASH.to_owned()),
            |record| (record.sequence, record.hash.clone()),
        ))
    }
}

/// The audit log: seals events into the chain and fans them out to subscribers.
#[derive(Debug)]
pub struct AuditLog {
    sink: Arc<dyn AuditSink>,
    /// Chain tip, guarded so concurrent emitters cannot interleave sequence
    /// numbers. Held across the sink write, which is what makes the on-disk
    /// order match the hash order.
    tip: Mutex<(u64, String)>,
    broadcast: broadcast::Sender<Arc<Event>>,
}

impl AuditLog {
    /// Open a log over a sink, continuing that sink's existing chain.
    ///
    /// # Errors
    ///
    /// Returns [`AuditError::Sink`] if the sink's tip cannot be read.
    pub async fn open(sink: Arc<dyn AuditSink>) -> Result<Self, AuditError> {
        let tip = sink.tip().await?;
        let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(Self {
            sink,
            tip: Mutex::new(tip),
            broadcast,
        })
    }

    /// A log that keeps everything in memory. Convenience for tests.
    ///
    /// # Errors
    ///
    /// Cannot fail in practice; the signature matches [`Self::open`].
    pub async fn in_memory() -> Result<(Self, Arc<InMemorySink>), AuditError> {
        let sink = Arc::new(InMemorySink::new());
        let log = Self::open(sink.clone()).await?;
        Ok((log, sink))
    }

    /// Subscribe to the live event stream.
    ///
    /// Subscribers receive events emitted after they subscribe. Historical
    /// events come from the durable log, not from here.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.broadcast.subscribe()
    }

    /// How many live subscribers there are.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.broadcast.receiver_count()
    }

    /// Seal an event into the chain, persist it, then broadcast it.
    ///
    /// Ordering matters: the durable write happens before the broadcast, so a
    /// subscriber can never observe an event that was not recorded. A broadcast
    /// with no subscribers is not an error.
    ///
    /// # Errors
    ///
    /// Returns [`AuditError`] if serialisation or the durable write fails.
    pub async fn record(&self, event: Event) -> Result<AuditRecord, AuditError> {
        let mut tip = self.tip.lock().await;
        let (last_sequence, last_hash) = &*tip;
        let record = AuditRecord::seal(&event, last_sequence + 1, last_hash)?;

        self.sink.append(&record).await?;
        *tip = (record.sequence, record.hash.clone());
        drop(tip);

        // `send` errors only when nobody is listening, which is normal.
        let _ = self.broadcast.send(Arc::new(event));
        Ok(record)
    }

    /// Record an event payload with no context attached.
    ///
    /// # Errors
    ///
    /// As [`Self::record`].
    pub async fn record_payload(&self, payload: AgentEvent) -> Result<AuditRecord, AuditError> {
        self.record(Event::new(payload)).await
    }

    /// The current chain tip.
    pub async fn tip(&self) -> (u64, String) {
        self.tip.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::ids::AgentId;

    use super::*;

    fn started(objective: &str) -> Event {
        Event::new(AgentEvent::TaskStarted {
            objective: objective.to_owned(),
            attempt: 1,
        })
    }

    #[tokio::test]
    async fn records_are_chained_in_order() {
        let (log, sink) = AuditLog::in_memory().await.unwrap();
        for i in 0..4 {
            log.record(started(&format!("o{i}"))).await.unwrap();
        }

        let records = sink.records().await;
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].prev_hash, GENESIS_HASH);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[3].sequence, 4);
        assert!(verify_chain(&records).is_intact());
    }

    #[tokio::test]
    async fn reopening_continues_the_existing_chain() {
        let sink = Arc::new(InMemorySink::new());
        {
            let log = AuditLog::open(sink.clone()).await.unwrap();
            log.record(started("first")).await.unwrap();
            log.record(started("second")).await.unwrap();
        }

        // A restart must not start a second chain from genesis.
        let reopened = AuditLog::open(sink.clone()).await.unwrap();
        reopened.record(started("third")).await.unwrap();

        let records = sink.records().await;
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].sequence, 3);
        assert!(verify_chain(&records).is_intact());
    }

    #[tokio::test]
    async fn subscribers_receive_events() {
        let (log, _sink) = AuditLog::in_memory().await.unwrap();
        let mut subscriber = log.subscribe();

        log.record(started("watched")).await.unwrap();

        let event = subscriber.recv().await.unwrap();
        assert_eq!(event.kind(), "agent.task.started");
    }

    #[tokio::test]
    async fn recording_works_with_no_subscribers() {
        let (log, sink) = AuditLog::in_memory().await.unwrap();
        assert_eq!(log.subscriber_count(), 0);
        log.record(started("unwatched")).await.unwrap();
        assert_eq!(sink.records().await.len(), 1);
    }

    #[tokio::test]
    async fn context_is_preserved_on_the_record() {
        let (log, sink) = AuditLog::in_memory().await.unwrap();
        let agent = AgentId::new();
        log.record(started("o").for_agent(agent)).await.unwrap();

        let records = sink.records().await;
        assert_eq!(records[0].agent_id, Some(agent));
        assert_eq!(records[0].kind, "agent.task.started");
    }

    #[tokio::test]
    async fn concurrent_writers_produce_a_valid_chain() {
        // Sequence numbers and back-pointers must stay consistent even when
        // several tasks emit at once; otherwise verification would fail on a
        // perfectly honest log and the whole mechanism would be noise.
        let (log, sink) = AuditLog::in_memory().await.unwrap();
        let log = Arc::new(log);

        let mut handles = Vec::new();
        for i in 0..16 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                log.record(started(&format!("concurrent {i}"))).await
            }));
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        let records = sink.records().await;
        assert_eq!(records.len(), 16);
        let verification = verify_chain(&records);
        assert!(verification.is_intact(), "{:?}", verification.breaks);
    }

    #[tokio::test]
    async fn in_memory_sink_can_be_queried_by_kind() {
        let (log, sink) = AuditLog::in_memory().await.unwrap();
        log.record(started("o")).await.unwrap();
        log.record_payload(AgentEvent::UnknownToolRequested {
            tool: "nope".into(),
        })
        .await
        .unwrap();

        assert!(sink.contains_kind("tool.unknown").await);
        assert!(!sink.contains_kind("approval.granted").await);
        assert_eq!(sink.records_of_kind("agent.task.started").await.len(), 1);
    }
}
