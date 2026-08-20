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
    async fn append(&self, record: &AuditRecord) -> Result<(), AuditError> {
        sqlx::query(
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
        .await
        .map_err(|error| AuditError::Sink(error.to_string()))?;
        Ok(())
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
        assert!(sink.append(&clash).await.is_err());
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
