//! Schedules, and the edges between tasks.
//!
//! Both live here because they answer the same question from two directions:
//! what is allowed to start, and when.

use agentos_core::ids::{AgentId, ScheduleId, TaskId};
use agentos_core::schedule::{Schedule, ScheduleStatus};
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_id, read_json, read_optional_id, read_optional_time, read_time, write_json,
    write_optional_time, write_time,
};
use crate::error::DbError;

const TABLE: &str = "schedules";

/// Reads and writes schedules.
#[derive(Debug, Clone)]
pub struct ScheduleRepository {
    pool: SqlitePool,
}

impl ScheduleRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a schedule.
    ///
    /// # Errors
    ///
    /// [`DbError::Conflict`] if the name is taken; [`DbError::Sql`] otherwise.
    pub async fn insert(&self, schedule: &Schedule) -> Result<(), DbError> {
        let result = sqlx::query(
            "INSERT INTO schedules (id, agent_id, name, objective, cadence, status,
                                    next_run_at, last_run_at, last_task_id,
                                    created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(schedule.id.to_string())
        .bind(schedule.agent_id.to_string())
        .bind(&schedule.name)
        .bind(&schedule.objective)
        .bind(write_json("cadence", &schedule.cadence)?)
        .bind(schedule.status.as_str())
        .bind(write_optional_time(schedule.next_run_at.as_ref()))
        .bind(write_optional_time(schedule.last_run_at.as_ref()))
        .bind(schedule.last_task_id.map(|id| id.to_string()))
        .bind(write_time(&schedule.created_at))
        .bind(write_time(&schedule.updated_at))
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(DbError::Conflict {
                    entity: "schedule",
                    value: schedule.name.clone(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Fetch a schedule.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: ScheduleId) -> Result<Schedule, DbError> {
        let row = sqlx::query("SELECT * FROM schedules WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(hydrate)
            .transpose()?
            .ok_or(DbError::NotFound {
                entity: "schedule",
                id: id.to_string(),
            })
    }

    /// Fetch a schedule by its name.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find_by_name(&self, name: &str) -> Result<Option<Schedule>, DbError> {
        let row = sqlx::query("SELECT * FROM schedules WHERE name = ?1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(hydrate).transpose()
    }

    /// Every schedule, newest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list(&self) -> Result<Vec<Schedule>, DbError> {
        let rows = sqlx::query("SELECT * FROM schedules ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Active schedules whose next occurrence has arrived, oldest first.
    ///
    /// Oldest first so a scheduler that can only start two things starts the two
    /// that have been waiting longest, rather than whichever the database
    /// happened to return.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_due(&self, limit: i64) -> Result<Vec<Schedule>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM schedules
              WHERE status = 'active' AND next_run_at IS NOT NULL AND next_run_at <= ?1
              ORDER BY next_run_at
              LIMIT ?2",
        )
        .bind(write_time(&agentos_core::now()))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Overwrite a schedule's mutable fields.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn update(&self, schedule: &Schedule) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE schedules
                SET objective = ?2, cadence = ?3, status = ?4, next_run_at = ?5,
                    last_run_at = ?6, last_task_id = ?7, updated_at = ?8
              WHERE id = ?1",
        )
        .bind(schedule.id.to_string())
        .bind(&schedule.objective)
        .bind(write_json("cadence", &schedule.cadence)?)
        .bind(schedule.status.as_str())
        .bind(write_optional_time(schedule.next_run_at.as_ref()))
        .bind(write_optional_time(schedule.last_run_at.as_ref()))
        .bind(schedule.last_task_id.map(|id| id.to_string()))
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                id: schedule.id.to_string(),
            });
        }
        Ok(())
    }

    /// Pause or resume a schedule.
    ///
    /// Resuming a schedule whose occurrence passed while it was paused does not
    /// fire for every slot it missed — the caller supplies the next occurrence,
    /// computed forward from now.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn set_status(
        &self,
        id: ScheduleId,
        status: ScheduleStatus,
        next_run_at: Option<agentos_core::Timestamp>,
    ) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE schedules SET status = ?2, next_run_at = ?3, updated_at = ?4 WHERE id = ?1",
        )
        .bind(id.to_string())
        .bind(status.as_str())
        .bind(write_optional_time(next_run_at.as_ref()))
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Delete a schedule.
    ///
    /// The tasks it already created are left alone: they are history, and a
    /// schedule being removed does not mean the work never happened.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn delete(&self, id: ScheduleId) -> Result<(), DbError> {
        let affected = sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "schedule",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Schedules belonging to one agent.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_agent(&self, agent_id: AgentId) -> Result<Vec<Schedule>, DbError> {
        let rows =
            sqlx::query("SELECT * FROM schedules WHERE agent_id = ?1 ORDER BY created_at DESC")
                .bind(agent_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(hydrate).collect()
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Schedule, DbError> {
    Ok(Schedule {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        agent_id: read_id(
            TABLE,
            "agent_id",
            row.try_get::<String, _>("agent_id")?.as_str(),
        )?,
        name: row.try_get("name")?,
        objective: row.try_get("objective")?,
        cadence: read_json(
            TABLE,
            "cadence",
            row.try_get::<String, _>("cadence")?.as_str(),
        )?,
        status: crate::convert::read_enum(
            TABLE,
            "status",
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        next_run_at: read_optional_time(TABLE, "next_run_at", row.try_get("next_run_at")?)?,
        last_run_at: read_optional_time(TABLE, "last_run_at", row.try_get("last_run_at")?)?,
        last_task_id: read_optional_id(TABLE, "last_task_id", row.try_get("last_task_id")?)?,
        created_at: read_time(
            TABLE,
            "created_at",
            row.try_get::<String, _>("created_at")?.as_str(),
        )?,
        updated_at: read_time(
            TABLE,
            "updated_at",
            row.try_get::<String, _>("updated_at")?.as_str(),
        )?,
    })
}

/// Reads and writes the edges of a task graph.
#[derive(Debug, Clone)]
pub struct DependencyRepository {
    pool: SqlitePool,
}

impl DependencyRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record that `task` waits for `depends_on`.
    ///
    /// Idempotent: adding the same edge twice is not an error, because a caller
    /// re-declaring a graph should not have to diff it first.
    ///
    /// This does **not** check for cycles — see
    /// [`Runtime::add_dependency`](../../agentos_runtime/struct.Runtime.html),
    /// which does, and which is the only thing that should be calling this.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if either task is absent, or if the two are the same.
    pub async fn add(&self, task: TaskId, depends_on: TaskId) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (task_id, depends_on_task_id) DO NOTHING",
        )
        .bind(task.to_string())
        .bind(depends_on.to_string())
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// What `task` is waiting for.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn dependencies_of(&self, task: TaskId) -> Result<Vec<TaskId>, DbError> {
        let rows = sqlx::query(
            "SELECT depends_on_task_id FROM task_dependencies
              WHERE task_id = ?1 ORDER BY created_at",
        )
        .bind(task.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                read_id(
                    "task_dependencies",
                    "depends_on_task_id",
                    row.try_get::<String, _>("depends_on_task_id")?.as_str(),
                )
            })
            .collect()
    }

    /// What is waiting for `task`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn dependents_of(&self, task: TaskId) -> Result<Vec<TaskId>, DbError> {
        let rows = sqlx::query(
            "SELECT task_id FROM task_dependencies
              WHERE depends_on_task_id = ?1 ORDER BY created_at",
        )
        .bind(task.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                read_id(
                    "task_dependencies",
                    "task_id",
                    row.try_get::<String, _>("task_id")?.as_str(),
                )
            })
            .collect()
    }

    /// Every edge, as `(task, depends_on)` pairs.
    ///
    /// Used to check a proposed edge for cycles without issuing one query per
    /// hop. Task graphs here are hand-authored and small; if that stops being
    /// true this becomes a recursive CTE.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn all(&self) -> Result<Vec<(TaskId, TaskId)>, DbError> {
        let rows = sqlx::query("SELECT task_id, depends_on_task_id FROM task_dependencies")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                Ok((
                    read_id(
                        "task_dependencies",
                        "task_id",
                        row.try_get::<String, _>("task_id")?.as_str(),
                    )?,
                    read_id(
                        "task_dependencies",
                        "depends_on_task_id",
                        row.try_get::<String, _>("depends_on_task_id")?.as_str(),
                    )?,
                ))
            })
            .collect()
    }

    /// Remove every edge out of `task`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn clear_for(&self, task: TaskId) -> Result<(), DbError> {
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = ?1")
            .bind(task.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::schedule::{Cadence, Clock};
    use agentos_core::task::Task;

    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    async fn seeded() -> (Database, AgentId) {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("worker");
        db.agents().insert(&agent).await.unwrap();
        (db, agent.id)
    }

    fn at(text: &str) -> agentos_core::Timestamp {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[tokio::test]
    async fn round_trips_a_schedule_including_its_cadence() {
        let (db, agent_id) = seeded().await;
        let schedule = Schedule::new(
            agent_id,
            "morning-review",
            "Review overdue follow-ups.",
            Cadence::Cron {
                expression: "0 9 * * MON-FRI".to_owned(),
                clock: Clock::Local,
            },
            at("2026-09-01T09:00:00Z"),
        )
        .unwrap();

        db.schedules().insert(&schedule).await.unwrap();
        let loaded = db.schedules().get(schedule.id).await.unwrap();
        assert_eq!(loaded, schedule);
    }

    #[tokio::test]
    async fn names_are_unique() {
        let (db, agent_id) = seeded().await;
        let first = Schedule::new(
            agent_id,
            "nightly",
            "Do it.",
            Cadence::Once,
            at("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        let second = Schedule::new(
            agent_id,
            "nightly",
            "Do it again.",
            Cadence::Once,
            at("2026-09-01T00:00:00Z"),
        )
        .unwrap();

        db.schedules().insert(&first).await.unwrap();
        let error = db.schedules().insert(&second).await.unwrap_err();
        assert!(matches!(
            error,
            DbError::Conflict {
                entity: "schedule",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn only_active_schedules_whose_time_has_come_are_due() {
        let (db, agent_id) = seeded().await;

        let past = Schedule::new(
            agent_id,
            "past",
            "Now.",
            Cadence::Once,
            at("2020-01-01T00:00:00Z"),
        )
        .unwrap();
        let future = Schedule::new(
            agent_id,
            "future",
            "Later.",
            Cadence::Once,
            at("2999-01-01T00:00:00Z"),
        )
        .unwrap();
        let mut paused = Schedule::new(
            agent_id,
            "paused",
            "Never.",
            Cadence::Once,
            at("2020-01-01T00:00:00Z"),
        )
        .unwrap();
        paused.status = ScheduleStatus::Paused;

        for schedule in [&past, &future, &paused] {
            db.schedules().insert(schedule).await.unwrap();
        }

        let due = db.schedules().list_due(10).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "past");
    }

    #[tokio::test]
    async fn deleting_a_schedule_keeps_the_work_it_already_did() {
        let (db, agent_id) = seeded().await;
        let mut schedule = Schedule::new(
            agent_id,
            "nightly",
            "Do it.",
            Cadence::Once,
            at("2020-01-01T00:00:00Z"),
        )
        .unwrap();
        db.schedules().insert(&schedule).await.unwrap();

        let task = Task::new(agent_id, "Do it.").from_schedule(schedule.id);
        db.tasks().insert(&task).await.unwrap();
        schedule.record_firing(agentos_core::now(), task.id);
        db.schedules().update(&schedule).await.unwrap();

        db.schedules().delete(schedule.id).await.unwrap();

        // The task survives, still carrying which schedule produced it.
        let loaded = db.tasks().get(task.id).await.unwrap();
        assert_eq!(loaded.schedule_id, Some(schedule.id));
    }

    #[tokio::test]
    async fn a_task_waits_for_its_dependencies_and_then_does_not() {
        let (db, agent_id) = seeded().await;
        let first = Task::new(agent_id, "Gather.");
        let second = Task::new(agent_id, "Summarise.").blocked();
        db.tasks().insert(&first).await.unwrap();
        db.tasks().insert(&second).await.unwrap();
        db.dependencies().add(second.id, first.id).await.unwrap();

        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert_eq!(runnable.len(), 1);
        assert_eq!(runnable[0].id, first.id);

        db.tasks()
            .set_status(first.id, agentos_core::TaskStatus::Succeeded)
            .await
            .unwrap();

        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert_eq!(runnable.len(), 1);
        assert_eq!(
            runnable[0].id, second.id,
            "the dependency succeeded, so the wait is over"
        );
    }

    #[tokio::test]
    async fn a_fan_in_waits_for_every_branch() {
        let (db, agent_id) = seeded().await;
        let a = Task::new(agent_id, "A.");
        let b = Task::new(agent_id, "B.");
        let join = Task::new(agent_id, "Both.").blocked();
        for task in [&a, &b, &join] {
            db.tasks().insert(task).await.unwrap();
        }
        db.dependencies().add(join.id, a.id).await.unwrap();
        db.dependencies().add(join.id, b.id).await.unwrap();

        db.tasks()
            .set_status(a.id, agentos_core::TaskStatus::Succeeded)
            .await
            .unwrap();

        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert!(
            !runnable.iter().any(|task| task.id == join.id),
            "one branch of two is not both branches"
        );

        db.tasks()
            .set_status(b.id, agentos_core::TaskStatus::Succeeded)
            .await
            .unwrap();
        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert!(runnable.iter().any(|task| task.id == join.id));
    }

    #[tokio::test]
    async fn a_failed_dependency_makes_its_dependents_unreachable() {
        let (db, agent_id) = seeded().await;
        let first = Task::new(agent_id, "Gather.");
        let second = Task::new(agent_id, "Summarise.").blocked();
        db.tasks().insert(&first).await.unwrap();
        db.tasks().insert(&second).await.unwrap();
        db.dependencies().add(second.id, first.id).await.unwrap();

        db.tasks()
            .set_status(first.id, agentos_core::TaskStatus::Failed)
            .await
            .unwrap();

        assert!(db.tasks().list_runnable(10).await.unwrap().is_empty());
        let stuck = db.tasks().list_unreachable(10).await.unwrap();
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].id, second.id);
    }

    #[tokio::test]
    async fn a_task_held_until_later_is_not_runnable_yet() {
        let (db, agent_id) = seeded().await;
        let later = Task::new(agent_id, "Later.").scheduled_for(at("2999-01-01T00:00:00Z"));
        let now = Task::new(agent_id, "Now.").scheduled_for(at("2020-01-01T00:00:00Z"));
        db.tasks().insert(&later).await.unwrap();
        db.tasks().insert(&now).await.unwrap();

        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert_eq!(runnable.len(), 1);
        assert_eq!(runnable[0].id, now.id);
        assert_eq!(runnable[0].scheduled_for, Some(at("2020-01-01T00:00:00Z")));
    }

    #[tokio::test]
    async fn edges_are_idempotent_and_readable_from_both_ends() {
        let (db, agent_id) = seeded().await;
        let first = Task::new(agent_id, "A.");
        let second = Task::new(agent_id, "B.");
        db.tasks().insert(&first).await.unwrap();
        db.tasks().insert(&second).await.unwrap();

        db.dependencies().add(second.id, first.id).await.unwrap();
        db.dependencies().add(second.id, first.id).await.unwrap();

        assert_eq!(
            db.dependencies().dependencies_of(second.id).await.unwrap(),
            vec![first.id]
        );
        assert_eq!(
            db.dependencies().dependents_of(first.id).await.unwrap(),
            vec![second.id]
        );
        assert_eq!(db.dependencies().all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_task_cannot_depend_on_itself() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "A.");
        db.tasks().insert(&task).await.unwrap();
        assert!(db.dependencies().add(task.id, task.id).await.is_err());
    }
}
