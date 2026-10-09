//! Tasks: objectives, independent of any attempt at them.

use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::{Task, TaskRun, TaskStatus};
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
        insert_with(&self.pool, task).await
    }

    /// Create a task together with the tasks it waits for, as one write.
    ///
    /// [`Self::list_runnable`] counts a `blocked` task as runnable once nothing
    /// it waits for is unfinished, and a task whose edges are not written yet
    /// waits for nothing. Written one after the other, the row and its edges
    /// leave a moment in which a scheduler in another process can start the
    /// task before it has been told to wait. In one transaction there is no
    /// such moment, and an edge that cannot be written leaves no task behind.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the agent or a dependency does not exist or the
    /// write fails; nothing is written then.
    pub async fn insert_waiting(&self, task: &Task, depends_on: &[TaskId]) -> Result<(), DbError> {
        let mut transaction = self.pool.begin().await?;
        insert_with(&mut *transaction, task).await?;
        for dependency in depends_on {
            crate::schedules::add_with(&mut *transaction, task.id, *dependency).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Claim a task for a run that is about to start, if it is still as the
    /// caller read it.
    ///
    /// Compare-and-set: the task becomes `running` only if its status is still
    /// `read`, and the answer says whether this caller won. Two schedulers, or
    /// a scheduler and a person pressing retry, can both have read the task as
    /// runnable; exactly one of their claims takes effect, and the other is
    /// told so before it has started anything.
    ///
    /// The comparison is with what the caller read, not with every status a
    /// run may start from. A scheduler reads a task as pending; if the
    /// operator cancels it, or a run fails it, before the scheduler's claim
    /// lands, that claim must lose, or a read a moment old re-runs unattended
    /// a task somebody has just stopped. A run may start from `pending` and
    /// `blocked`, which is a task nobody has run, and from `failed` and
    /// `cancelled`, which is a retry; any other `read` claims nothing.
    ///
    /// A task nobody has run is claimed only if [`Self::list_runnable`] would
    /// still list it: its time has come and everything it waits for has
    /// succeeded. A dependency added after the read, which leaves the status
    /// `blocked` either way, therefore takes the claim away as well. A retry
    /// is the operator's decision about a task that has already started once,
    /// and is not held to either.
    ///
    /// `false` also when the task does not exist: there is nothing to start.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn claim(&self, id: TaskId, read: TaskStatus) -> Result<bool, DbError> {
        claim_with(&self.pool, id, read).await
    }

    /// Claim a task as [`Self::claim`] does, and write the run that will
    /// carry it out, as one write.
    ///
    /// Written one after the other, a process that died between the two
    /// would leave a task `running` with no run: never reaped, because the
    /// reaper starts from unfinished runs, and never claimable again, because
    /// nothing claims a running task. In one transaction the task is running
    /// exactly when it has a run, and a claim that loses writes no run.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the run cannot be written; the claim is undone then.
    pub async fn claim_for_run(&self, read: TaskStatus, run: &TaskRun) -> Result<bool, DbError> {
        let mut transaction = self.pool.begin().await?;
        if !claim_with(&mut *transaction, run.task_id, read).await? {
            return Ok(false);
        }
        crate::runs::insert_with(&mut *transaction, run).await?;
        transaction.commit().await?;
        Ok(true)
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

    /// Recent tasks in one status, most recently settled into it first.
    ///
    /// Selected by the store, so a rare status is found however many tasks in
    /// other states were created since. Ordered by when the task last reached
    /// a terminal status rather than when it was created: a retry of an old
    /// task that failed again today is today's failure. A task in a status
    /// that is not terminal has no such time and sorts by its creation.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_with_status(
        &self,
        status: TaskStatus,
        limit: i64,
    ) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM tasks WHERE status = ?1
              ORDER BY COALESCE(completed_at, created_at) DESC LIMIT ?2",
        )
        .bind(status.as_str())
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

    /// Tasks that could be started right now.
    ///
    /// A task qualifies when the clock permits it and every task it depends on
    /// has succeeded. Both halves are evaluated here in SQL rather than by
    /// reading the graph into memory, so a scheduler asking "what now?" pays for
    /// one query however wide the graph is.
    ///
    /// `blocked` rows are included: whether a dependency is satisfied is a fact
    /// about the database at this instant, not a status somebody has to remember
    /// to update.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_runnable(&self, limit: i64) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query(
            "SELECT t.* FROM tasks t
              WHERE t.status IN ('pending', 'blocked')
                AND (t.scheduled_for IS NULL OR t.scheduled_for <= ?1)
                AND NOT EXISTS (
                      SELECT 1 FROM task_dependencies d
                        JOIN tasks p ON p.id = d.depends_on_task_id
                       WHERE d.task_id = t.id
                         AND p.status <> 'succeeded'
                    )
              ORDER BY COALESCE(t.scheduled_for, t.created_at)
              LIMIT ?2",
        )
        .bind(write_time(&agentos_core::now()))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Tasks that can never run, because something they depend on will not
    /// succeed.
    ///
    /// Reported rather than swept, so the caller decides what a dead branch
    /// means. Leaving them blocked forever would be a silent hang.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_unreachable(&self, limit: i64) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query(
            "SELECT t.* FROM tasks t
              WHERE t.status IN ('pending', 'blocked')
                AND EXISTS (
                      SELECT 1 FROM task_dependencies d
                        JOIN tasks p ON p.id = d.depends_on_task_id
                       WHERE d.task_id = t.id
                         AND p.status IN ('failed', 'cancelled')
                    )
              ORDER BY t.created_at
              LIMIT ?1",
        )
        .bind(limit)
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

/// Claim a task through any executor. See [`TaskRepository::claim`].
async fn claim_with<'e, E>(executor: E, id: TaskId, read: TaskStatus) -> Result<bool, DbError>
where
    E: sqlx::SqliteExecutor<'e>,
{
    if !matches!(
        read,
        TaskStatus::Pending | TaskStatus::Blocked | TaskStatus::Failed | TaskStatus::Cancelled
    ) {
        return Ok(false);
    }
    // The second half of the condition is `list_runnable`'s, so a claim on a
    // task nobody has run holds it to what the scheduler's read promised.
    let affected = sqlx::query(
        "UPDATE tasks
            SET status = 'running',
                started_at = COALESCE(started_at, ?2),
                completed_at = NULL
          WHERE id = ?1
            AND status = ?3
            AND (status IN ('failed', 'cancelled')
                 OR ((scheduled_for IS NULL OR scheduled_for <= ?2)
                     AND NOT EXISTS (
                           SELECT 1 FROM task_dependencies d
                             JOIN tasks p ON p.id = d.depends_on_task_id
                            WHERE d.task_id = tasks.id
                              AND p.status <> 'succeeded'
                         )))",
    )
    .bind(id.to_string())
    .bind(write_time(&agentos_core::now()))
    .bind(read.as_str())
    .execute(executor)
    .await?
    .rows_affected();
    Ok(affected == 1)
}

/// Insert a task through any executor.
///
/// Shared with the schedule repository, which creates a firing's task inside
/// the same transaction that advances the schedule.
pub(crate) async fn insert_with<'e, E>(executor: E, task: &Task) -> Result<(), DbError>
where
    E: sqlx::SqliteExecutor<'e>,
{
    sqlx::query(
        "INSERT INTO tasks (id, agent_id, objective, status, parent_task_id,
                                scheduled_for, schedule_id, created_at, started_at,
                                completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(task.id.to_string())
    .bind(task.agent_id.to_string())
    .bind(&task.objective)
    .bind(task.status.as_str())
    .bind(task.parent_task_id.map(|id| id.to_string()))
    .bind(write_optional_time(task.scheduled_for.as_ref()))
    .bind(task.schedule_id.map(|id| id.to_string()))
    .bind(write_time(&task.created_at))
    .bind(write_optional_time(task.started_at.as_ref()))
    .bind(write_optional_time(task.completed_at.as_ref()))
    .execute(executor)
    .await?;
    Ok(())
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
        scheduled_for: read_optional_time(TABLE, "scheduled_for", row.try_get("scheduled_for")?)?,
        schedule_id: read_optional_id(TABLE, "schedule_id", row.try_get("schedule_id")?)?,
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
    async fn a_task_and_what_it_waits_for_are_written_together() {
        let (db, agent_id) = seeded().await;
        let first = Task::new(agent_id, "Draft the summary");
        db.tasks().insert(&first).await.unwrap();

        let waiting = Task::new(agent_id, "Send it").blocked();
        db.tasks()
            .insert_waiting(&waiting, &[first.id])
            .await
            .unwrap();
        // Never runnable on its own: the edge arrived with the row.
        let runnable = db.tasks().list_runnable(10).await.unwrap();
        assert!(runnable.iter().all(|task| task.id != waiting.id));
        assert_eq!(
            db.dependencies().dependencies_of(waiting.id).await.unwrap(),
            vec![first.id]
        );

        // An edge to a task that does not exist takes the task down with it,
        // rather than leaving a task that waits for nothing.
        let orphan = Task::new(agent_id, "Waits for a ghost").blocked();
        assert!(
            db.tasks()
                .insert_waiting(&orphan, &[first.id, TaskId::new()])
                .await
                .is_err()
        );
        assert!(db.tasks().find(orphan.id).await.unwrap().is_none());
        assert!(
            db.dependencies()
                .dependencies_of(orphan.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn of_two_concurrent_claims_exactly_one_wins() {
        // Two pools over one file stand in for two processes, each with its
        // own connections; an in-memory database has only one connection and
        // would serialise the race away.
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("agentos.db");
        let first = Database::open(&path).await.unwrap();
        let second = Database::open(&path).await.unwrap();
        let agent = sample_agent("worker");
        first.agents().insert(&agent).await.unwrap();
        let (mine, theirs) = (first.tasks(), second.tasks());

        for round in 0..20 {
            let task = Task::new(agent.id, format!("contended {round}"));
            mine.insert(&task).await.unwrap();
            let (a, b) = tokio::join!(
                mine.claim(task.id, TaskStatus::Pending),
                theirs.claim(task.id, TaskStatus::Pending)
            );
            let winners = [a.unwrap(), b.unwrap()]
                .into_iter()
                .filter(|won| *won)
                .count();
            assert_eq!(winners, 1, "round {round}");
            assert_eq!(
                first.tasks().get(task.id).await.unwrap().status,
                TaskStatus::Running
            );
        }
    }

    #[tokio::test]
    async fn a_claim_starts_only_what_may_be_started() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "o");
        db.tasks().insert(&task).await.unwrap();

        assert!(
            db.tasks()
                .claim(task.id, TaskStatus::Pending)
                .await
                .unwrap()
        );
        // Running already: a second attempt would interleave two runs, whatever
        // the caller believes it read.
        assert!(
            !db.tasks()
                .claim(task.id, TaskStatus::Pending)
                .await
                .unwrap()
        );
        assert!(
            !db.tasks()
                .claim(task.id, TaskStatus::Running)
                .await
                .unwrap()
        );

        // Succeeded: running it again is a new task, not a claim on this one.
        db.tasks()
            .set_status(task.id, TaskStatus::Succeeded)
            .await
            .unwrap();
        assert!(
            !db.tasks()
                .claim(task.id, TaskStatus::Succeeded)
                .await
                .unwrap()
        );

        // Failed: a retry that read the failure may claim it, and the finish
        // time goes with the failure it described.
        db.tasks()
            .set_status(task.id, TaskStatus::Failed)
            .await
            .unwrap();
        assert!(db.tasks().claim(task.id, TaskStatus::Failed).await.unwrap());
        assert!(
            db.tasks()
                .get(task.id)
                .await
                .unwrap()
                .completed_at
                .is_none()
        );

        assert!(
            !db.tasks()
                .claim(TaskId::new(), TaskStatus::Pending)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn a_claim_on_a_stale_read_loses() {
        let (db, agent_id) = seeded().await;

        // Read as pending by a scheduler, then cancelled by the operator
        // before the scheduler's claim lands: the claim must not undo the
        // cancellation by running it.
        let cancelled = Task::new(agent_id, "Stopped by the operator");
        db.tasks().insert(&cancelled).await.unwrap();
        db.tasks()
            .set_status(cancelled.id, TaskStatus::Cancelled)
            .await
            .unwrap();
        assert!(
            !db.tasks()
                .claim(cancelled.id, TaskStatus::Pending)
                .await
                .unwrap()
        );
        assert_eq!(
            db.tasks().get(cancelled.id).await.unwrap().status,
            TaskStatus::Cancelled
        );

        // Read as runnable, then told to wait for something unfinished. The
        // status says blocked either way, so only the dependency check tells
        // the two reads apart.
        let first = Task::new(agent_id, "Gather");
        let waiting = Task::new(agent_id, "Summarise").blocked();
        db.tasks().insert(&first).await.unwrap();
        db.tasks().insert(&waiting).await.unwrap();
        assert!(
            db.tasks()
                .list_runnable(10)
                .await
                .unwrap()
                .iter()
                .any(|task| task.id == waiting.id)
        );
        db.dependencies().add(waiting.id, first.id).await.unwrap();
        assert!(
            !db.tasks()
                .claim(waiting.id, TaskStatus::Blocked)
                .await
                .unwrap()
        );

        // A task whose time has not come is not claimed as one that is due.
        let later = Task::new(agent_id, "Tomorrow")
            .scheduled_for(agentos_core::now() + std::time::Duration::from_secs(3600));
        db.tasks().insert(&later).await.unwrap();
        assert!(
            !db.tasks()
                .claim(later.id, TaskStatus::Pending)
                .await
                .unwrap()
        );

        // Once what it waits for has succeeded, the same claim wins.
        db.tasks()
            .set_status(first.id, TaskStatus::Succeeded)
            .await
            .unwrap();
        assert!(
            db.tasks()
                .claim(waiting.id, TaskStatus::Blocked)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn a_claim_and_its_run_are_written_together() {
        let (db, agent_id) = seeded().await;
        let task = Task::new(agent_id, "o");
        db.tasks().insert(&task).await.unwrap();

        let run = TaskRun::new(task.id, 1);
        assert!(
            db.tasks()
                .claim_for_run(TaskStatus::Pending, &run)
                .await
                .unwrap()
        );
        assert_eq!(
            db.tasks().get(task.id).await.unwrap().status,
            TaskStatus::Running
        );
        assert_eq!(db.runs().list_for_task(task.id).await.unwrap().len(), 1);

        // A lost claim writes no run.
        let second = TaskRun::new(task.id, 2);
        assert!(
            !db.tasks()
                .claim_for_run(TaskStatus::Pending, &second)
                .await
                .unwrap()
        );
        assert_eq!(db.runs().list_for_task(task.id).await.unwrap().len(), 1);

        // A run that cannot be written takes the claim with it: the attempt
        // number is taken, and the task is left as it was rather than running
        // with no run behind it.
        db.tasks()
            .set_status(task.id, TaskStatus::Failed)
            .await
            .unwrap();
        let clash = TaskRun::new(task.id, 1);
        assert!(
            db.tasks()
                .claim_for_run(TaskStatus::Failed, &clash)
                .await
                .is_err()
        );
        assert_eq!(
            db.tasks().get(task.id).await.unwrap().status,
            TaskStatus::Failed
        );
        assert_eq!(db.runs().list_for_task(task.id).await.unwrap().len(), 1);
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
    async fn tasks_in_a_status_are_found_behind_newer_ones() {
        let (db, agent_id) = seeded().await;
        let failed = Task::new(agent_id, "the one that failed");
        db.tasks().insert(&failed).await.unwrap();
        db.tasks()
            .set_status(failed.id, TaskStatus::Failed)
            .await
            .unwrap();
        for i in 0..5 {
            db.tasks()
                .insert(&Task::new(agent_id, format!("later {i}")))
                .await
                .unwrap();
        }

        // Out of reach of the newest few tasks of every status.
        assert!(
            db.tasks()
                .list(5)
                .await
                .unwrap()
                .iter()
                .all(|task| task.id != failed.id)
        );
        let found = db
            .tasks()
            .list_with_status(TaskStatus::Failed, 5)
            .await
            .unwrap();
        assert_eq!(
            found.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![failed.id]
        );

        // Five newer tasks fail, and then the old one fails again on a retry.
        // That is the freshest failure, and is listed first.
        let later: Vec<TaskId> = db
            .tasks()
            .list(5)
            .await
            .unwrap()
            .iter()
            .map(|task| task.id)
            .collect();
        for id in &later {
            db.tasks()
                .set_status(*id, TaskStatus::Failed)
                .await
                .unwrap();
        }
        db.tasks()
            .set_status(failed.id, TaskStatus::Running)
            .await
            .unwrap();
        db.tasks()
            .set_status(failed.id, TaskStatus::Failed)
            .await
            .unwrap();
        let found = db
            .tasks()
            .list_with_status(TaskStatus::Failed, 5)
            .await
            .unwrap();
        assert_eq!(found.len(), 5);
        assert_eq!(found[0].id, failed.id);
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
