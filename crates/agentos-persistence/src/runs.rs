//! Task runs: individual attempts at a task.

use agentos_core::ids::{TaskId, TaskRunId};
use agentos_core::task::{TaskFailure, TaskRun, TaskState};
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_enum, read_id, read_optional_json, read_optional_time, read_time, write_json,
    write_optional_time, write_time,
};
use crate::error::DbError;

const TABLE: &str = "task_runs";

/// Reads and writes task runs.
#[derive(Debug, Clone)]
pub struct RunRepository {
    pool: SqlitePool,
}

impl RunRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record the start of a run.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the task does not exist or the attempt number is taken.
    pub async fn insert(&self, run: &TaskRun) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO task_runs (id, task_id, attempt, state, tainted, steps_taken,
                                    result, failure, input_tokens, output_tokens,
                                    started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(run.id.to_string())
        .bind(run.task_id.to_string())
        .bind(i64::from(run.attempt))
        .bind(run.state.as_str())
        .bind(i64::from(run.tainted))
        .bind(i64::from(run.steps_taken))
        .bind(run.result.as_deref())
        .bind(
            run.failure
                .as_ref()
                .map(|failure| write_json("failure", failure))
                .transpose()?,
        )
        .bind(clamp_u64(run.input_tokens))
        .bind(clamp_u64(run.output_tokens))
        .bind(write_time(&run.started_at))
        .bind(write_optional_time(run.completed_at.as_ref()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persist the current state of a run.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn update(&self, run: &TaskRun) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE task_runs
                SET state = ?2, tainted = ?3, steps_taken = ?4, result = ?5, failure = ?6,
                    input_tokens = ?7, output_tokens = ?8, completed_at = ?9
              WHERE id = ?1",
        )
        .bind(run.id.to_string())
        .bind(run.state.as_str())
        .bind(i64::from(run.tainted))
        .bind(i64::from(run.steps_taken))
        .bind(run.result.as_deref())
        .bind(
            run.failure
                .as_ref()
                .map(|failure| write_json("failure", failure))
                .transpose()?,
        )
        .bind(clamp_u64(run.input_tokens))
        .bind(clamp_u64(run.output_tokens))
        .bind(write_optional_time(run.completed_at.as_ref()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "task run",
                id: run.id.to_string(),
            });
        }
        Ok(())
    }

    /// Fetch a run.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: TaskRunId) -> Result<TaskRun, DbError> {
        self.find(id).await?.ok_or(DbError::NotFound {
            entity: "task run",
            id: id.to_string(),
        })
    }

    /// Fetch a run, or `None`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(&self, id: TaskRunId) -> Result<Option<TaskRun>, DbError> {
        let row = sqlx::query("SELECT * FROM task_runs WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// All runs of a task, oldest attempt first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_task(&self, task_id: TaskId) -> Result<Vec<TaskRun>, DbError> {
        let rows = sqlx::query("SELECT * FROM task_runs WHERE task_id = ?1 ORDER BY attempt")
            .bind(task_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// The most recent run of a task.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn latest_for_task(&self, task_id: TaskId) -> Result<Option<TaskRun>, DbError> {
        let row =
            sqlx::query("SELECT * FROM task_runs WHERE task_id = ?1 ORDER BY attempt DESC LIMIT 1")
                .bind(task_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// The next attempt number for a task.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn next_attempt(&self, task_id: TaskId) -> Result<u32, DbError> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(attempt), 0) AS highest FROM task_runs WHERE task_id = ?1",
        )
        .bind(task_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        let highest: i64 = row.try_get("highest")?;
        Ok(u32::try_from(highest.saturating_add(1)).unwrap_or(u32::MAX))
    }

    /// Runs that are not in a terminal state.
    ///
    /// On startup these are runs a previous process abandoned, and the runtime
    /// marks them failed rather than leaving them looking alive forever.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_unfinished(&self) -> Result<Vec<TaskRun>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM task_runs
              WHERE state NOT IN ('completed', 'failed', 'cancelled')
              ORDER BY started_at",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }
}

fn clamp_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<TaskRun, DbError> {
    let attempt: i64 = row.try_get("attempt")?;
    let steps: i64 = row.try_get("steps_taken")?;
    let input_tokens: i64 = row.try_get("input_tokens")?;
    let output_tokens: i64 = row.try_get("output_tokens")?;

    Ok(TaskRun {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        task_id: read_id(
            TABLE,
            "task_id",
            row.try_get::<String, _>("task_id")?.as_str(),
        )?,
        attempt: u32::try_from(attempt).unwrap_or(u32::MAX),
        state: read_enum::<TaskState>(TABLE, "state", row.try_get::<String, _>("state")?.as_str())?,
        tainted: row.try_get::<i64, _>("tainted")? != 0,
        steps_taken: u32::try_from(steps).unwrap_or(u32::MAX),
        result: row.try_get("result")?,
        failure: read_optional_json::<TaskFailure>(TABLE, "failure", row.try_get("failure")?)?,
        input_tokens: u64::try_from(input_tokens).unwrap_or(0),
        output_tokens: u64::try_from(output_tokens).unwrap_or(0),
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
    use agentos_core::task::Task;

    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    async fn seeded() -> (Database, TaskId) {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("runner");
        db.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "objective");
        db.tasks().insert(&task).await.unwrap();
        (db, task.id)
    }

    #[tokio::test]
    async fn round_trips_a_run_with_a_failure() {
        let (db, task_id) = seeded().await;
        let mut run = TaskRun::new(task_id, 1);
        db.runs().insert(&run).await.unwrap();

        run.state = TaskState::Failed;
        run.tainted = true;
        run.steps_taken = 7;
        run.failure = Some(TaskFailure::StepBudgetExhausted { limit: 24 });
        run.input_tokens = 1234;
        run.output_tokens = 567;
        run.completed_at = Some(agentos_core::now());
        db.runs().update(&run).await.unwrap();

        let loaded = db.runs().get(run.id).await.unwrap();
        assert_eq!(loaded.state, TaskState::Failed);
        assert!(loaded.tainted);
        assert_eq!(loaded.steps_taken, 7);
        assert_eq!(
            loaded.failure,
            Some(TaskFailure::StepBudgetExhausted { limit: 24 })
        );
        assert_eq!(loaded.input_tokens, 1234);
        assert_eq!(loaded.output_tokens, 567);
        assert!(loaded.completed_at.is_some());
    }

    #[tokio::test]
    async fn attempts_increment_and_are_unique() {
        let (db, task_id) = seeded().await;
        assert_eq!(db.runs().next_attempt(task_id).await.unwrap(), 1);

        db.runs().insert(&TaskRun::new(task_id, 1)).await.unwrap();
        assert_eq!(db.runs().next_attempt(task_id).await.unwrap(), 2);

        // Re-using an attempt number would silently fork a task's history.
        assert!(db.runs().insert(&TaskRun::new(task_id, 1)).await.is_err());
    }

    #[tokio::test]
    async fn latest_run_is_the_highest_attempt() {
        let (db, task_id) = seeded().await;
        db.runs().insert(&TaskRun::new(task_id, 1)).await.unwrap();
        let second = TaskRun::new(task_id, 2);
        db.runs().insert(&second).await.unwrap();

        assert_eq!(
            db.runs()
                .latest_for_task(task_id)
                .await
                .unwrap()
                .map(|r| r.id),
            Some(second.id)
        );
        assert_eq!(db.runs().list_for_task(task_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn unfinished_runs_are_reported_for_recovery() {
        let (db, task_id) = seeded().await;
        let mut live = TaskRun::new(task_id, 1);
        live.state = TaskState::Executing;
        db.runs().insert(&live).await.unwrap();

        let mut done = TaskRun::new(task_id, 2);
        done.state = TaskState::Completed;
        db.runs().insert(&done).await.unwrap();

        let unfinished = db.runs().list_unfinished().await.unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].id, live.id);
    }

    #[tokio::test]
    async fn updating_a_missing_run_is_an_error() {
        let (db, task_id) = seeded().await;
        let err = db
            .runs()
            .update(&TaskRun::new(task_id, 9))
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::NotFound { .. }));
    }
}
