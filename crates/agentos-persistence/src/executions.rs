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

/// What one tool has been doing over a window, from its executions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolUsage {
    /// The tool.
    pub tool: String,
    /// Every call the model made to it, whatever became of it.
    pub calls: u64,
    /// Calls that reached the tool: it succeeded, failed or ran out of time.
    pub executed: u64,
    /// Calls the policy refused.
    pub denied: u64,
    /// Calls refused at the approval step: by a person, or by the run's
    /// approval budget without anyone being asked.
    pub approval_denied: u64,
    /// Calls that ran and did not succeed, timeouts included.
    pub failed: u64,
    /// Calls made while the run held untrusted data.
    pub under_taint: u64,
    /// Calls that were put to a person, whatever they answered.
    pub approvals: u64,
    /// Time spent across every call, in milliseconds.
    pub total_duration_ms: u64,
    /// When it was last called.
    pub last_used_at: Timestamp,
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

impl ExecutionRepository {
    /// What each tool has been doing since `since`, busiest first.
    ///
    /// One grouped query rather than a row per call. The window is bound as a
    /// string, and that is sound: every timestamp in this table is written by
    /// the same fixed-width RFC3339 UTC formatter at microsecond precision, so
    /// lexical order is chronological order. It also means
    /// `idx_executions_tool` still serves the scan, which a function applied to
    /// `started_at` would forfeit.
    ///
    /// The outcome literals are [`ToolOutcome::as_str`] spelled out, because
    /// SQL cannot call it. `executed` is [`ToolOutcome::executed`]; `failed`
    /// is the executed outcomes other than success. Invalid arguments and
    /// cancellations count only as calls, deliberately: the first never
    /// reached the policy and the second was the operator stopping the run, so
    /// neither is something the tool did or was refused. The tests insert one
    /// call of every outcome and check each lands where this says it should.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure, or [`DbError::CorruptRow`] if a stored
    /// timestamp does not parse.
    pub async fn usage_since(&self, since: Timestamp) -> Result<Vec<ToolUsage>, DbError> {
        let rows = sqlx::query(
            "SELECT tool,
                    COUNT(*) AS calls,
                    SUM(outcome IN ('success', 'failed', 'timed_out')) AS executed,
                    SUM(outcome = 'denied') AS denied,
                    SUM(outcome = 'approval_denied') AS approval_denied,
                    SUM(outcome IN ('failed', 'timed_out')) AS failed,
                    SUM(tainted <> 0) AS under_taint,
                    SUM(approval_id IS NOT NULL) AS approvals,
                    SUM(duration_ms) AS total_duration_ms,
                    MAX(started_at) AS last_used_at
               FROM tool_executions
              WHERE started_at >= ?1
              GROUP BY tool
              ORDER BY calls DESC, tool",
        )
        .bind(write_time(&since))
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .map(|row| {
                let count = |column: &str| -> Result<u64, DbError> {
                    Ok(u64::try_from(row.try_get::<i64, _>(column)?).unwrap_or(0))
                };
                Ok(ToolUsage {
                    tool: row.try_get("tool")?,
                    calls: count("calls")?,
                    executed: count("executed")?,
                    denied: count("denied")?,
                    approval_denied: count("approval_denied")?,
                    failed: count("failed")?,
                    under_taint: count("under_taint")?,
                    approvals: count("approvals")?,
                    total_duration_ms: count("total_duration_ms")?,
                    last_used_at: read_time(
                        TABLE,
                        "started_at",
                        row.try_get::<String, _>("last_used_at")?.as_str(),
                    )?,
                })
            })
            .collect()
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

    /// Where an outcome is counted in [`ToolUsage`], beyond `calls`.
    ///
    /// Exhaustive with no wildcard, so a new outcome does not compile until
    /// somebody decides where it is counted; the test below then checks the
    /// query agrees.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Bucket {
        Executed,
        Denied,
        ApprovalDenied,
        CallsOnly,
    }

    const fn bucket(outcome: ToolOutcome) -> Bucket {
        match outcome {
            ToolOutcome::Success | ToolOutcome::Failed | ToolOutcome::TimedOut => Bucket::Executed,
            ToolOutcome::Denied => Bucket::Denied,
            ToolOutcome::ApprovalDenied => Bucket::ApprovalDenied,
            ToolOutcome::InvalidArguments | ToolOutcome::Cancelled => Bucket::CallsOnly,
        }
    }

    const fn failed(outcome: ToolOutcome) -> bool {
        matches!(outcome, ToolOutcome::Failed | ToolOutcome::TimedOut)
    }

    #[test]
    fn the_buckets_agree_with_the_outcome_classification() {
        for &outcome in ToolOutcome::ALL {
            assert_eq!(
                bucket(outcome) == Bucket::Executed,
                outcome.executed(),
                "{}",
                outcome.as_str()
            );
            assert!(!failed(outcome) || outcome.executed());
        }
    }

    #[tokio::test]
    async fn usage_counts_every_outcome_where_the_classification_says() {
        // One call of every outcome. If the literals in the query drift from
        // `ToolOutcome::as_str`, or an outcome is added without a bucket, the
        // totals stop matching what the Rust side expects. `ALL` is declared
        // with the enum, so a new outcome is in it without anyone remembering.
        let (db, run_id) = seeded().await;
        for &outcome in ToolOutcome::ALL {
            db.executions()
                .insert(&record(run_id, "t", outcome, Effect::Allow))
                .await
                .unwrap();
        }

        let usage = db
            .executions()
            .usage_since(agentos_core::now() - chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
        let t = &usage[0];
        let expect = |wanted: Bucket| -> u64 {
            ToolOutcome::ALL
                .iter()
                .filter(|outcome| bucket(**outcome) == wanted)
                .count() as u64
        };
        assert_eq!(t.calls, ToolOutcome::ALL.len() as u64);
        assert_eq!(t.executed, expect(Bucket::Executed));
        assert_eq!(t.denied, expect(Bucket::Denied));
        assert_eq!(t.approval_denied, expect(Bucket::ApprovalDenied));
        assert_eq!(
            t.failed,
            ToolOutcome::ALL
                .iter()
                .filter(|outcome| failed(**outcome))
                .count() as u64
        );
        assert_eq!(
            t.calls,
            t.executed + t.denied + t.approval_denied + expect(Bucket::CallsOnly)
        );
    }

    #[tokio::test]
    async fn a_denied_and_an_executed_call_land_in_different_buckets() {
        let (db, run_id) = seeded().await;
        let mut executed = record(run_id, "terminal.exec", ToolOutcome::Success, Effect::Ask);
        executed.approval_id = Some(ApprovalId::new());
        executed.tainted = false;
        db.executions().insert(&executed).await.unwrap();
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
                "filesystem.read",
                ToolOutcome::Success,
                Effect::Allow,
            ))
            .await
            .unwrap();

        let usage = db
            .executions()
            .usage_since(agentos_core::now() - chrono::Duration::hours(1))
            .await
            .unwrap();
        // Busiest first.
        assert_eq!(usage[0].tool, "terminal.exec");
        let exec = &usage[0];
        assert_eq!(exec.calls, 2);
        assert_eq!(exec.executed, 1);
        assert_eq!(exec.denied, 1);
        assert_eq!(exec.approvals, 1);
        assert_eq!(exec.under_taint, 1);
        assert_eq!(exec.total_duration_ms, 34);
        assert_eq!(usage[1].tool, "filesystem.read");
        assert_eq!(usage[1].calls, 1);
    }

    #[tokio::test]
    async fn calls_before_the_window_are_excluded() {
        let (db, run_id) = seeded().await;
        let mut old = record(run_id, "old.tool", ToolOutcome::Success, Effect::Allow);
        old.started_at = agentos_core::now() - chrono::Duration::days(8);
        db.executions().insert(&old).await.unwrap();
        let recent = record(run_id, "terminal.exec", ToolOutcome::Success, Effect::Allow);
        db.executions().insert(&recent).await.unwrap();

        let usage = db
            .executions()
            .usage_since(agentos_core::now() - chrono::Duration::days(7))
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].tool, "terminal.exec");
        assert_eq!(
            agentos_core::format_timestamp(&usage[0].last_used_at),
            agentos_core::format_timestamp(&recent.started_at)
        );
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
