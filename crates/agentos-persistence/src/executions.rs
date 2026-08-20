//! Tool executions: what was attempted, what was decided, what happened.
//!
//! This table is the answer to "what did the agent actually do, and who let it?".
//! Every row records the permission effect and the taint state at the moment of
//! the call, so a later reader does not have to reconstruct them from the policy
//! as it stands today.

use agentos_core::Timestamp;
use agentos_core::ids::{ApprovalId, TaskRunId, ToolExecutionId};
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolOutcome;
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_enum, read_id, read_optional_id, read_optional_time, read_time, read_unit_enum,
    write_json, write_optional_time, write_time,
};
use crate::error::DbError;

const TABLE: &str = "tool_executions";

/// A persisted tool execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionRecord {
    /// Identity.
    pub id: ToolExecutionId,
    /// The run it belongs to.
    pub run_id: TaskRunId,
    /// The tool.
    pub tool: String,
    /// The provider's call identifier.
    pub call_id: String,
    /// Validated arguments.
    pub arguments: serde_json::Value,
    /// How it ended.
    pub outcome: ToolOutcome,
    /// What the policy engine decided.
    pub effect: Effect,
    /// Assessed risk at the time of the call.
    pub risk: RiskLevel,
    /// Whether the run had ingested untrusted data.
    pub tainted: bool,
    /// The approval that gated it, if any.
    pub approval_id: Option<ApprovalId>,
    /// Bytes of output produced.
    pub output_bytes: u64,
    /// Error text, when it failed.
    pub error: Option<String>,
    /// How long it took.
    pub duration_ms: u64,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished.
    pub completed_at: Option<Timestamp>,
}

/// Reads and writes tool executions.
#[derive(Debug, Clone)]
pub struct ExecutionRepository {
    pool: SqlitePool,
}

impl ExecutionRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record an execution.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the run does not exist.
    pub async fn insert(&self, record: &ToolExecutionRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO tool_executions (id, run_id, tool, call_id, arguments, outcome,
                                          effect, risk, tainted, approval_id, output_bytes,
                                          error, duration_ms, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )
        .bind(record.id.to_string())
        .bind(record.run_id.to_string())
        .bind(&record.tool)
        .bind(&record.call_id)
        .bind(write_json("arguments", &record.arguments)?)
        .bind(record.outcome.as_str())
        .bind(record.effect.as_str())
        .bind(record.risk.as_str())
        .bind(i64::from(record.tainted))
        .bind(record.approval_id.map(|id| id.to_string()))
        .bind(i64::try_from(record.output_bytes).unwrap_or(i64::MAX))
        .bind(record.error.as_deref())
        .bind(i64::try_from(record.duration_ms).unwrap_or(i64::MAX))
        .bind(write_time(&record.started_at))
        .bind(write_optional_time(record.completed_at.as_ref()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Executions in a run, oldest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_run(
        &self,
        run_id: TaskRunId,
    ) -> Result<Vec<ToolExecutionRecord>, DbError> {
        let rows =
            sqlx::query("SELECT * FROM tool_executions WHERE run_id = ?1 ORDER BY started_at")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Executions that were refused, newest first.
    ///
    /// Surfaced on the dashboard: a burst of denials is the signal that
    /// something is trying to do what it should not.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_denied(&self, limit: i64) -> Result<Vec<ToolExecutionRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM tool_executions
              WHERE outcome IN ('denied', 'approval_denied', 'invalid_arguments')
              ORDER BY started_at DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<ToolExecutionRecord, DbError> {
    let output_bytes: i64 = row.try_get("output_bytes")?;
    let duration_ms: i64 = row.try_get("duration_ms")?;

    Ok(ToolExecutionRecord {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        run_id: read_id(
            TABLE,
            "run_id",
            row.try_get::<String, _>("run_id")?.as_str(),
        )?,
        tool: row.try_get("tool")?,
        call_id: row.try_get("call_id")?,
        arguments: crate::convert::read_json(
            TABLE,
            "arguments",
            row.try_get::<String, _>("arguments")?.as_str(),
        )?,
        outcome: read_unit_enum::<ToolOutcome>(
            TABLE,
            "outcome",
            row.try_get::<String, _>("outcome")?.as_str(),
        )?,
        effect: read_unit_enum::<Effect>(
            TABLE,
            "effect",
            row.try_get::<String, _>("effect")?.as_str(),
        )?,
        risk: read_enum::<RiskLevel>(TABLE, "risk", row.try_get::<String, _>("risk")?.as_str())?,
        tainted: row.try_get::<i64, _>("tainted")? != 0,
        approval_id: read_optional_id::<ApprovalId>(
            TABLE,
            "approval_id",
            row.try_get("approval_id")?,
        )?,
        output_bytes: u64::try_from(output_bytes).unwrap_or(0),
        error: row.try_get("error")?,
        duration_ms: u64::try_from(duration_ms).unwrap_or(0),
        started_at: read_time(
            TABLE,
            "started_at",
            row.try_get::<String, _>("started_at")?.as_str(),
        )?,
        completed_at: read_optional_time(TABLE, "completed_at", row.try_get("completed_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    use agentos_core::task::{Task, TaskRun};

    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    async fn seeded() -> (Database, TaskRunId) {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("executor");
        db.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "o");
        db.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        db.runs().insert(&run).await.unwrap();
        (db, run.id)
    }

    fn record(
        run_id: TaskRunId,
        tool: &str,
        outcome: ToolOutcome,
        effect: Effect,
    ) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: ToolExecutionId::new(),
            run_id,
            tool: tool.to_owned(),
            call_id: "call-1".to_owned(),
            arguments: serde_json::json!({"path": "/tmp/x"}),
            outcome,
            effect,
            risk: RiskLevel::Medium,
            tainted: true,
            approval_id: None,
            output_bytes: 42,
            error: None,
            duration_ms: 17,
            started_at: agentos_core::now(),
            completed_at: Some(agentos_core::now()),
        }
    }

    #[tokio::test]
    async fn round_trips_an_execution() {
        let (db, run_id) = seeded().await;
        let approval = ApprovalId::new();
        let mut written = record(
            run_id,
            "filesystem.write",
            ToolOutcome::Success,
            Effect::Ask,
        );
        written.approval_id = Some(approval);

        db.executions().insert(&written).await.unwrap();
        let loaded = db.executions().list_for_run(run_id).await.unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], written);
        assert_eq!(loaded[0].approval_id, Some(approval));
        assert!(loaded[0].tainted);
    }

    #[tokio::test]
    async fn denials_are_queryable() {
        let (db, run_id) = seeded().await;
        db.executions()
            .insert(&record(
                run_id,
                "filesystem.read",
                ToolOutcome::Success,
                Effect::Allow,
            ))
            .await
            .unwrap();
        db.executions()
            .insert(&record(
                run_id,
                "terminal.exec",
                ToolOutcome::Denied,
                Effect::Deny,
            ))
            .await
            .unwrap();
        db.executions()
            .insert(&record(
                run_id,
                "email.send",
                ToolOutcome::ApprovalDenied,
                Effect::Ask,
            ))
            .await
            .unwrap();

        let denied = db.executions().list_denied(10).await.unwrap();
        assert_eq!(denied.len(), 2);
        assert!(denied.iter().all(|e| !e.outcome.executed()));
    }

    #[tokio::test]
    async fn every_outcome_round_trips() {
        let (db, run_id) = seeded().await;
        let outcomes = [
            ToolOutcome::Success,
            ToolOutcome::InvalidArguments,
            ToolOutcome::Denied,
            ToolOutcome::ApprovalDenied,
            ToolOutcome::Cancelled,
            ToolOutcome::Failed,
            ToolOutcome::TimedOut,
        ];
        for outcome in outcomes {
            db.executions()
                .insert(&record(run_id, "t", outcome, Effect::Allow))
                .await
                .unwrap();
        }

        let loaded = db.executions().list_for_run(run_id).await.unwrap();
        assert_eq!(loaded.len(), outcomes.len());
    }
}
