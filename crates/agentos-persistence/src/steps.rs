//! Execution trace steps.

use agentos_core::ids::{TaskRunId, TaskStepId, ToolExecutionId};
use agentos_core::task::{TaskState, TaskStep, TaskStepKind};
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_enum, read_id, read_optional_id, read_optional_json, read_time, read_unit_enum,
    write_json, write_time,
};
use crate::error::DbError;

const TABLE: &str = "task_steps";

/// Reads and writes the per-run execution trace.
#[derive(Debug, Clone)]
pub struct StepRepository {
    pool: SqlitePool,
}

impl StepRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Append a step.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the run does not exist or the ordinal is taken.
    pub async fn insert(&self, step: &TaskStep) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO task_steps (id, run_id, ordinal, kind, state, summary,
                                     tool_execution_id, detail, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(step.id.to_string())
        .bind(step.run_id.to_string())
        .bind(i64::from(step.ordinal))
        .bind(kind_str(step.kind))
        .bind(step.state.as_str())
        .bind(&step.summary)
        .bind(step.tool_execution_id.map(|id| id.to_string()))
        .bind(
            step.detail
                .as_ref()
                .map(|detail| write_json("detail", detail))
                .transpose()?,
        )
        .bind(write_time(&step.at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The trace of a run, in order.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_run(&self, run_id: TaskRunId) -> Result<Vec<TaskStep>, DbError> {
        let rows = sqlx::query("SELECT * FROM task_steps WHERE run_id = ?1 ORDER BY ordinal")
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// The next ordinal for a run.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn next_ordinal(&self, run_id: TaskRunId) -> Result<u32, DbError> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(ordinal), 0) AS highest FROM task_steps WHERE run_id = ?1",
        )
        .bind(run_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        let highest: i64 = row.try_get("highest")?;
        Ok(u32::try_from(highest.saturating_add(1)).unwrap_or(u32::MAX))
    }
}

const fn kind_str(kind: TaskStepKind) -> &'static str {
    match kind {
        TaskStepKind::Planning => "planning",
        TaskStepKind::ToolCall => "tool_call",
        TaskStepKind::Approval => "approval",
        TaskStepKind::Verification => "verification",
        TaskStepKind::Recovery => "recovery",
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<TaskStep, DbError> {
    let ordinal: i64 = row.try_get("ordinal")?;
    Ok(TaskStep {
        id: read_id::<TaskStepId>(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        run_id: read_id(
            TABLE,
            "run_id",
            row.try_get::<String, _>("run_id")?.as_str(),
        )?,
        ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        kind: read_unit_enum::<TaskStepKind>(
            TABLE,
            "kind",
            row.try_get::<String, _>("kind")?.as_str(),
        )?,
        state: read_enum::<TaskState>(TABLE, "state", row.try_get::<String, _>("state")?.as_str())?,
        summary: row.try_get("summary")?,
        tool_execution_id: read_optional_id::<ToolExecutionId>(
            TABLE,
            "tool_execution_id",
            row.try_get("tool_execution_id")?,
        )?,
        detail: read_optional_json(TABLE, "detail", row.try_get("detail")?)?,
        at: read_time(TABLE, "at", row.try_get::<String, _>("at")?.as_str())?,
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
        let agent = sample_agent("tracer");
        db.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "o");
        db.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        db.runs().insert(&run).await.unwrap();
        (db, run.id)
    }

    fn step(run_id: TaskRunId, ordinal: u32, kind: TaskStepKind) -> TaskStep {
        TaskStep {
            id: TaskStepId::new(),
            run_id,
            ordinal,
            kind,
            state: TaskState::Planning,
            summary: format!("step {ordinal}"),
            tool_execution_id: None,
            detail: Some(serde_json::json!({"ordinal": ordinal})),
            at: agentos_core::now(),
        }
    }

    #[tokio::test]
    async fn steps_are_returned_in_order() {
        let (db, run_id) = seeded().await;
        for ordinal in [3, 1, 2] {
            db.steps()
                .insert(&step(run_id, ordinal, TaskStepKind::Planning))
                .await
                .unwrap();
        }

        let trace = db.steps().list_for_run(run_id).await.unwrap();
        assert_eq!(
            trace.iter().map(|s| s.ordinal).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn every_kind_round_trips() {
        let (db, run_id) = seeded().await;
        let kinds = [
            TaskStepKind::Planning,
            TaskStepKind::ToolCall,
            TaskStepKind::Approval,
            TaskStepKind::Verification,
            TaskStepKind::Recovery,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            let ordinal = u32::try_from(index).unwrap_or(0) + 1;
            db.steps()
                .insert(&step(run_id, ordinal, *kind))
                .await
                .unwrap();
        }

        let loaded = db.steps().list_for_run(run_id).await.unwrap();
        assert_eq!(
            loaded.iter().map(|s| s.kind).collect::<Vec<_>>(),
            kinds.to_vec()
        );
        assert_eq!(loaded[0].detail, Some(serde_json::json!({"ordinal": 1})));
    }

    #[tokio::test]
    async fn ordinals_increment_and_are_unique_per_run() {
        let (db, run_id) = seeded().await;
        assert_eq!(db.steps().next_ordinal(run_id).await.unwrap(), 1);

        db.steps()
            .insert(&step(run_id, 1, TaskStepKind::Planning))
            .await
            .unwrap();
        assert_eq!(db.steps().next_ordinal(run_id).await.unwrap(), 2);

        assert!(
            db.steps()
                .insert(&step(run_id, 1, TaskStepKind::Planning))
                .await
                .is_err()
        );
    }
}
