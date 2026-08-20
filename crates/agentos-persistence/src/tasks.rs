//! Tasks: objectives, independent of any attempt at them.

use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::{Task, TaskStatus};
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_id, read_optional_id, read_optional_time, read_time, write_optional_time, write_time,
};
use crate::error::DbError;

const TABLE: &str = "tasks";

/// Reads and writes tasks.
#[derive(Debug, Clone)]
pub struct TaskRepository {
    pool: SqlitePool,
}

impl TaskRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a task.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the agent does not exist or the write fails.
    pub async fn insert(&self, task: &Task) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO tasks (id, agent_id, objective, status, parent_task_id,
                                created_at, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(task.id.to_string())
        .bind(task.agent_id.to_string())
        .bind(&task.objective)
        .bind(task.status.as_str())
        .bind(task.parent_task_id.map(|id| id.to_string()))
        .bind(write_time(&task.created_at))
        .bind(write_optional_time(task.started_at.as_ref()))
        .bind(write_optional_time(task.completed_at.as_ref()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch a task.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: TaskId) -> Result<Task, DbError> {
        self.find(id).await?.ok_or(DbError::NotFound {
            entity: "task",
            id: id.to_string(),
        })
    }

    /// Fetch a task, or `None`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(&self, id: TaskId) -> Result<Option<Task>, DbError> {
        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// Recent tasks, newest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list(&self, limit: i64) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query("SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?1")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Recent tasks for one agent, newest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_agent(
        &self,
        agent_id: AgentId,
        limit: i64,
    ) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM tasks WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )
        .bind(agent_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Tasks whose latest run is still going.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_active(&self) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query("SELECT * FROM tasks WHERE status = 'running' ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Update a task's status and lifecycle timestamps.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn set_status(&self, id: TaskId, status: TaskStatus) -> Result<(), DbError> {
        let now = agentos_core::now();
        let started = matches!(status, TaskStatus::Running).then(|| write_time(&now));
        let completed = matches!(
            status,
            TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Cancelled
        )
        .then(|| write_time(&now));

        let affected = sqlx::query(
            "UPDATE tasks
                SET status = ?2,
                    started_at = COALESCE(started_at, ?3),
                    completed_at = ?4
              WHERE id = ?1",
        )
        .bind(id.to_string())
        .bind(status.as_str())
        .bind(started)
        .bind(completed)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "task",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Direct children of a task, for orchestrated graphs.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn children(&self, parent: TaskId) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query("SELECT * FROM tasks WHERE parent_task_id = ?1 ORDER BY created_at")
            .bind(parent.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Task, DbError> {
    Ok(Task {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        agent_id: read_id(
            TABLE,
            "agent_id",
            row.try_get::<String, _>("agent_id")?.as_str(),
        )?,
        objective: row.try_get("objective")?,
        status: crate::convert::read_enum(
            TABLE,
            "status",
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        parent_task_id: read_optional_id(TABLE, "parent_task_id", row.try_get("parent_task_id")?)?,
        created_at: read_time(
            TABLE,
            "created_at",
            row.try_get::<String, _>("created_at")?.as_str(),
        )?,
        started_at: read_optional_time(TABLE, "started_at", row.try_get("started_at")?)?,
        completed_at: read_optional_time(TABLE, "completed_at", row.try_get("completed_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    async fn seeded() -> (Database, AgentId) {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("worker");
        db.agents().insert(&agent).await.unwrap();
        (db, agent.id)
    }

    #[tokio::test]
    async fn round_trips_a_task() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "Review overdue follow-ups");
        db.tasks().insert(&task).await.unwrap();

        let loaded = db.tasks().get(task.id).await.unwrap();
        assert_eq!(loaded.objective, "Review overdue follow-ups");
        assert_eq!(loaded.status, TaskStatus::Pending);
        assert!(loaded.started_at.is_none());
    }

    #[tokio::test]
    async fn status_transitions_stamp_timestamps_once() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "o");
        db.tasks().insert(&task).await.unwrap();

        db.tasks()
            .set_status(task.id, TaskStatus::Running)
            .await
            .unwrap();
        let running = db.tasks().get(task.id).await.unwrap();
        let first_start = running.started_at.unwrap();
        assert!(running.completed_at.is_none());

        db.tasks()
            .set_status(task.id, TaskStatus::Succeeded)
            .await
            .unwrap();
        let done = db.tasks().get(task.id).await.unwrap();
        // `started_at` records the first start, not the latest write.
        assert_eq!(done.started_at.unwrap(), first_start);
        assert!(done.completed_at.is_some());
        assert_eq!(done.status, TaskStatus::Succeeded);
    }

    #[tokio::test]
    async fn lists_are_scoped_and_ordered() {
        let (db, agent_id) = seeded().await;
        for i in 0..3 {
            db.tasks()
                .insert(&Task::new(agent_id, format!("task {i}")))
                .await
                .unwrap();
        }

        assert_eq!(db.tasks().list(10).await.unwrap().len(), 3);
        assert_eq!(db.tasks().list(2).await.unwrap().len(), 2);
        assert_eq!(
            db.tasks().list_for_agent(agent_id, 10).await.unwrap().len(),
            3
        );
        assert_eq!(
            db.tasks()
                .list_for_agent(AgentId::new(), 10)
                .await
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn active_tasks_are_filtered_by_status() {
        let (db, agent_id) = seeded().await;
        let running = Task::new(agent_id, "running");
        let idle = Task::new(agent_id, "idle");
        db.tasks().insert(&running).await.unwrap();
        db.tasks().insert(&idle).await.unwrap();
        db.tasks()
            .set_status(running.id, TaskStatus::Running)
            .await
            .unwrap();

        let active = db.tasks().list_active().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, running.id);
    }

    #[tokio::test]
    async fn child_tasks_are_linked() {
        let (db, agent_id) = seeded().await;
        let parent = Task::new(agent_id, "parent");
        db.tasks().insert(&parent).await.unwrap();
        let child = Task::new(agent_id, "child").with_parent(parent.id);
        db.tasks().insert(&child).await.unwrap();

        let children = db.tasks().children(parent.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].parent_task_id, Some(parent.id));
    }

    #[tokio::test]
    async fn deleting_an_agent_cascades_to_its_tasks() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "doomed");
        db.tasks().insert(&task).await.unwrap();

        db.agents().delete(agent_id).await.unwrap();
        assert!(db.tasks().find(task.id).await.unwrap().is_none());
    }
}
