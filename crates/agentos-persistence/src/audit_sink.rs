//! SQLite-backed audit sink.
//!
//! The `audit_events` table carries `BEFORE UPDATE` and `BEFORE DELETE` triggers
//! that abort. That is what makes the log append-only: not this code, and not a
//! convention that future contributors will honour.

use agentos_audit::{AuditError, AuditRecord, AuditSink, GENESIS_HASH};
use agentos_core::ids::{AgentId, EventId, TaskId, TaskRunId};
use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::convert::{read_id, read_optional_id, read_time, write_time};
use crate::error::DbError;

const TABLE: &str = "audit_events";

/// Writes audit records to SQLite.
#[derive(Debug, Clone)]
pub struct SqliteAuditSink {
    pool: SqlitePool,
}

impl SqliteAuditSink {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The most recent records, newest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn tail(&self, limit: i64) -> Result<Vec<AuditRecord>, DbError> {
        let rows = sqlx::query("SELECT * FROM audit_events ORDER BY sequence DESC LIMIT ?1")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// The most recent records of the given kinds, newest first.
    ///
    /// Selected by the store rather than filtered from [`Self::tail`]: records
    /// of a rare kind sit far apart in a busy log, and the newest `limit`
    /// records of every kind may hold none of them.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn tail_of_kinds(
        &self,
        kinds: &[&str],
        limit: i64,
    ) -> Result<Vec<AuditRecord>, DbError> {
        // The kinds travel as one bound JSON array, so the statement is fixed
        // text and nothing a caller passes is ever part of it.
        let kinds = serde_json::Value::from(kinds.to_vec()).to_string();
        let rows = sqlx::query(
            "SELECT * FROM audit_events
              WHERE kind IN (SELECT value FROM json_each(?1))
              ORDER BY sequence DESC LIMIT ?2",
        )
        .bind(kinds)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Every record in chain order, for verification.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn all(&self) -> Result<Vec<AuditRecord>, DbError> {
        let rows = sqlx::query("SELECT * FROM audit_events ORDER BY sequence")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Records belonging to one run, in chain order.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn for_run(&self, run_id: TaskRunId) -> Result<Vec<AuditRecord>, DbError> {
        let rows = sqlx::query("SELECT * FROM audit_events WHERE run_id = ?1 ORDER BY sequence")
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// One record, by the identity it shares with its event.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(&self, id: EventId) -> Result<Option<AuditRecord>, DbError> {
        let row = sqlx::query("SELECT * FROM audit_events WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// Records after a given position, in chain order.
    ///
    /// What lets a verification carry on from where the last one stopped
    /// rather than rehash the whole log each time.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn after(&self, sequence: u64) -> Result<Vec<AuditRecord>, DbError> {
        let rows = sqlx::query("SELECT * FROM audit_events WHERE sequence > ?1 ORDER BY sequence")
            .bind(i64::try_from(sequence).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// How many records the log holds.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn count(&self) -> Result<i64, DbError> {
        let row = sqlx::query("SELECT COUNT(*) AS total FROM audit_events")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("total")?)
    }
}

#[async_trait]
impl AuditSink for SqliteAuditSink {
    /// Append a record, refusing it as a conflict if its position is taken.
    ///
    /// The desktop application and a terminal can each hold a log over this
    /// database, and each caches the tip it last wrote. Positions are dense, so
    /// a log whose tip is stale always offers a position another process has
    /// already filled, and the unique index on `sequence` refuses it in the
    /// same statement that would have written it: there is no moment between
    /// the check and the write for a third writer to use. The log then reseals
    /// the record after the one that won.
    ///
    /// The sink checks nothing else about the record. Whether it links to its
    /// predecessor is the verifier's question, and a store that quietly refused
    /// unlinked records would hide exactly the tampering the verifier exists to
    /// report.
    async fn append(&self, record: &AuditRecord) -> Result<(), AuditError> {
        let result = sqlx::query(
            "INSERT INTO audit_events (id, sequence, at, kind, agent_id, task_id, run_id,
                                       payload, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(record.id.to_string())
        .bind(i64::try_from(record.sequence).unwrap_or(i64::MAX))
        .bind(write_time(&record.at))
        .bind(&record.kind)
        .bind(record.agent_id.map(|id| id.to_string()))
        .bind(record.task_id.map(|id| id.to_string()))
        .bind(record.run_id.map(|id| id.to_string()))
        .bind(record.payload.to_string())
        .bind(&record.prev_hash)
        .bind(&record.hash)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            // The event id is unique too, but a log never offers one event
            // twice; only the position can be taken by somebody else.
            Err(sqlx::Error::Database(error))
                if error.is_unique_violation() && error.message().contains("sequence") =>
            {
                Err(AuditError::Conflict {
                    sequence: record.sequence,
                })
            }
            Err(error) => Err(AuditError::Sink(error.to_string())),
        }
    }

    async fn tip(&self) -> Result<(u64, String), AuditError> {
        let row =
            sqlx::query("SELECT sequence, hash FROM audit_events ORDER BY sequence DESC LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| AuditError::Sink(error.to_string()))?;

        match row {
            None => Ok((0, GENESIS_HASH.to_owned())),
            Some(row) => {
                let sequence: i64 = row
                    .try_get("sequence")
                    .map_err(|error| AuditError::Sink(error.to_string()))?;
                let hash: String = row
                    .try_get("hash")
                    .map_err(|error| AuditError::Sink(error.to_string()))?;
                Ok((u64::try_from(sequence).unwrap_or(0), hash))
            }
        }
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<AuditRecord, DbError> {
    let sequence: i64 = row.try_get("sequence")?;
    Ok(AuditRecord {
        id: read_id::<EventId>(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        sequence: u64::try_from(sequence).unwrap_or(0),
        at: read_time(TABLE, "at", row.try_get::<String, _>("at")?.as_str())?,
        kind: row.try_get("kind")?,
        agent_id: read_optional_id::<AgentId>(TABLE, "agent_id", row.try_get("agent_id")?)?,
        task_id: read_optional_id::<TaskId>(TABLE, "task_id", row.try_get("task_id")?)?,
        run_id: read_optional_id::<TaskRunId>(TABLE, "run_id", row.try_get("run_id")?)?,
        payload: crate::convert::read_json(
            TABLE,
            "payload",
            row.try_get::<String, _>("payload")?.as_str(),
        )?,
        prev_hash: row.try_get("prev_hash")?,
        hash: row.try_get("hash")?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agentos_audit::{AuditLog, verify_chain};
    use agentos_core::event::{AgentEvent, Event};

    use super::*;
    use crate::Database;

    fn started(objective: &str) -> Event {
        Event::new(AgentEvent::TaskStarted {
            objective: objective.to_owned(),
            attempt: 1,
        })
    }

    #[tokio::test]
    async fn records_of_chosen_kinds_are_found_however_far_back_they_are() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        log.record(Event::new(AgentEvent::ProviderKeySet {
            provider: "anthropic".to_owned(),
        }))
        .await
        .unwrap();
        log.record(Event::new(AgentEvent::ProviderKeyRemoved {
            provider: "anthropic".to_owned(),
        }))
        .await
        .unwrap();
        for i in 0..20 {
            log.record(started(&format!("objective {i}")))
                .await
                .unwrap();
        }

        let sink = db.audit_sink();
        // The newest records of every kind hold neither of them.
        assert!(
            sink.tail(10)
                .await
                .unwrap()
                .iter()
                .all(|record| record.kind == "agent.task.started")
        );

        let kinds = ["operator.provider_key.set", "operator.provider_key.removed"];
        let found = sink.tail_of_kinds(&kinds, 10).await.unwrap();
        let found: Vec<&str> = found.iter().map(|record| record.kind.as_str()).collect();
        assert_eq!(found, vec![kinds[1], kinds[0]], "newest first");

        assert_eq!(sink.tail_of_kinds(&kinds, 1).await.unwrap().len(), 1);
        assert!(sink.tail_of_kinds(&[], 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn records_persist_and_verify() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();

        for i in 0..5 {
            log.record(started(&format!("objective {i}")))
                .await
                .unwrap();
        }

        let records = db.audit_sink().all().await.unwrap();
        assert_eq!(records.len(), 5);
        let verification = verify_chain(&records);
        assert!(verification.is_intact(), "{:?}", verification.breaks);
    }

    #[tokio::test]
    async fn reopening_continues_the_chain() {
        let db = Database::in_memory().await.unwrap();
        {
            let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
            log.record(started("first")).await.unwrap();
        }
        let reopened = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        reopened.record(started("second")).await.unwrap();

        let records = db.audit_sink().all().await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].sequence, 2);
        assert!(verify_chain(&records).is_intact());
    }

    #[tokio::test]
    async fn a_nanosecond_precision_clock_does_not_break_the_chain() {
        // The bug this guards against shipped: the chain hashed a timestamp at
        // whatever precision the clock provided, while the database stored
        // microseconds. On Linux, whose clock reports nanoseconds, every record
        // read back as tampered — the audit log's entire purpose, broken, on a
        // platform the developer did not happen to be using.
        //
        // Constructing the timestamp explicitly makes the test fail everywhere
        // rather than only where the clock is precise enough to expose it.
        use chrono::TimeZone;

        let db = Database::in_memory().await.unwrap();
        let sink = db.audit_sink();

        let mut prev = GENESIS_HASH.to_owned();
        for (index, nanos) in [772_812_948u32, 1, 999_999_999, 0].into_iter().enumerate() {
            let mut event = started("nanosecond clock");
            event.at = chrono::Utc
                .timestamp_opt(1_700_000_000 + index as i64, nanos)
                .single()
                .expect("valid timestamp");

            let record =
                AuditRecord::seal(&event, u64::try_from(index).unwrap_or(0) + 1, &prev).unwrap();
            prev.clone_from(&record.hash);
            sink.append(&record).await.unwrap();
        }

        let reloaded = sink.all().await.unwrap();
        assert_eq!(reloaded.len(), 4);
        let verification = verify_chain(&reloaded);
        assert!(
            verification.is_intact(),
            "a precise clock must not make an untouched log look tampered: {:?}",
            verification.breaks
        );
    }

    #[tokio::test]
    async fn the_log_refuses_updates() {
        // The point of the trigger: even a direct SQL statement cannot rewrite
        // history. If this test ever fails, the audit log is decorative.
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        log.record(started("immutable")).await.unwrap();

        let err = sqlx::query("UPDATE audit_events SET kind = 'tampered' WHERE sequence = 1")
            .execute(db.pool())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("append-only"),
            "expected the trigger to abort, got: {err}"
        );

        let records = db.audit_sink().all().await.unwrap();
        assert_eq!(records[0].kind, "agent.task.started");
    }

    #[tokio::test]
    async fn the_log_refuses_deletes() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        log.record(started("immutable")).await.unwrap();

        let err = sqlx::query("DELETE FROM audit_events WHERE sequence = 1")
            .execute(db.pool())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("append-only"),
            "expected the trigger to abort, got: {err}"
        );
        assert_eq!(db.audit_sink().count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn duplicate_sequence_numbers_are_rejected() {
        let db = Database::in_memory().await.unwrap();
        let sink = db.audit_sink();
        let record = AuditRecord::seal(&started("a"), 1, GENESIS_HASH).unwrap();
        sink.append(&record).await.unwrap();

        let clash = AuditRecord::seal(&started("b"), 1, GENESIS_HASH).unwrap();
        assert!(matches!(
            sink.append(&clash).await,
            Err(AuditError::Conflict { sequence: 1 })
        ));
    }

    #[tokio::test]
    async fn two_logs_over_one_database_keep_one_chain() {
        // Two processes, each with its own log and its own cached tip. Two
        // pools over one file, so their writes really do race.
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("agentos.db");
        let desktop = Database::open(&path).await.unwrap();
        let terminal = Database::open(&path).await.unwrap();
        let desktop_log = Arc::new(
            AuditLog::open(Arc::new(desktop.audit_sink()))
                .await
                .unwrap(),
        );
        let terminal_log = Arc::new(
            AuditLog::open(Arc::new(terminal.audit_sink()))
                .await
                .unwrap(),
        );

        // Interleaved: each log's cached tip is stale on every write.
        for i in 0..5 {
            desktop_log
                .record(started(&format!("desktop {i}")))
                .await
                .unwrap();
            terminal_log
                .record(started(&format!("terminal {i}")))
                .await
                .unwrap();
        }

        // And at once.
        let mut writers = Vec::new();
        for i in 0..10 {
            for log in [desktop_log.clone(), terminal_log.clone()] {
                writers.push(tokio::spawn(async move {
                    log.record(started(&format!("concurrent {i}"))).await
                }));
            }
        }
        for writer in writers {
            writer.await.unwrap().unwrap();
        }

        let records = desktop.audit_sink().all().await.unwrap();
        assert_eq!(records.len(), 30, "a record was lost to a collision");
        let verification = verify_chain(&records);
        assert!(verification.is_intact(), "{:?}", verification.breaks);
    }

    #[tokio::test]
    async fn records_can_be_filtered_by_run() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        let run = TaskRunId::new();

        log.record(started("with run").for_run(run)).await.unwrap();
        log.record(started("without run")).await.unwrap();

        assert_eq!(db.audit_sink().for_run(run).await.unwrap().len(), 1);
        assert_eq!(db.audit_sink().tail(10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_record_found_by_id_comes_back_with_its_hashes_unchanged() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        log.record(started("first")).await.unwrap();
        let event = started("second").for_run(TaskRunId::new());
        let written = log.record(event.clone()).await.unwrap();

        let found = db.audit_sink().find(event.id).await.unwrap().unwrap();
        assert_eq!(found, written);
        assert_eq!(found.prev_hash, written.prev_hash);
        assert_eq!(found.hash, written.hash);
        assert!(found.is_intact());

        assert!(
            db.audit_sink()
                .find(EventId::new())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_record_with_no_agent_task_or_run_round_trips() {
        // Operator actions belong to no run. The envelope's context columns are
        // nullable for exactly this, and the hash covers their absence.
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        let event = Event::new(AgentEvent::ProviderKeySet {
            provider: "anthropic".into(),
        });
        let written = log.record(event.clone()).await.unwrap();

        let found = db.audit_sink().find(event.id).await.unwrap().unwrap();
        assert_eq!(found, written);
        assert!(found.agent_id.is_none() && found.task_id.is_none() && found.run_id.is_none());
        assert!(verify_chain(&db.audit_sink().all().await.unwrap()).is_intact());
    }

    #[tokio::test]
    async fn records_after_a_position_are_the_rest_of_the_chain() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        for i in 0..4 {
            log.record(started(&format!("o{i}"))).await.unwrap();
        }
        let rest = db.audit_sink().after(2).await.unwrap();
        assert_eq!(
            rest.iter().map(|r| r.sequence).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(db.audit_sink().after(4).await.unwrap().is_empty());
        assert_eq!(db.audit_sink().after(0).await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn tail_returns_newest_first() {
        let db = Database::in_memory().await.unwrap();
        let log = AuditLog::open(Arc::new(db.audit_sink())).await.unwrap();
        for i in 0..3 {
            log.record(started(&format!("o{i}"))).await.unwrap();
        }
        let tail = db.audit_sink().tail(2).await.unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].sequence, 3);
        assert_eq!(tail[1].sequence, 2);
    }
}
