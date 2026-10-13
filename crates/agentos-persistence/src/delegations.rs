//! Delegations: the edges between a run and the runs it asked other agents to
//! carry out.
//!
//! Thin, like every repository here. Whether a delegation may happen is the
//! runtime's decision and what the child may do is the policy engine's; this
//! records what was decided, in the order the runtime decided it.

use agentos_core::delegation::{Delegation, DelegationState};
use agentos_core::ids::{DelegationId, TaskId, TaskRunId};
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_enum, read_id, read_optional_id, read_optional_time, read_time, write_optional_time,
    write_time,
};
use crate::error::DbError;

const TABLE: &str = "delegations";

/// How many edges [`DelegationRepository::ancestry`] follows before it stops.
///
/// Far above any depth the runtime permits, so an honest tree is never cut
/// short. The bound exists for a corrupt one: two rows that name each other's
/// runs as parent and child would otherwise send the walk round for ever.
pub const MAX_ANCESTRY_HOPS: usize = 16;

/// Reads and writes delegations.
#[derive(Debug, Clone)]
pub struct DelegationRepository {
    pool: SqlitePool,
}

impl DelegationRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a delegation, before anything is done about it.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if a run, agent or task it names does not exist, or if
    /// another delegation already claims its child run. The caller must not
    /// perform a delegation this refused.
    pub async fn insert(&self, delegation: &Delegation) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO delegations (id, parent_run_id, parent_agent_id, child_agent_id,
                                      child_task_id, child_run_id, objective, depth, state,
                                      steps_spent, requested_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(delegation.id.to_string())
        .bind(delegation.parent_run_id.to_string())
        .bind(delegation.parent_agent_id.to_string())
        .bind(delegation.child_agent_id.to_string())
        .bind(delegation.child_task_id.to_string())
        .bind(delegation.child_run_id.map(|id| id.to_string()))
        .bind(&delegation.objective)
        .bind(i64::from(delegation.depth))
        .bind(delegation.state.as_str())
        .bind(i64::from(delegation.steps_spent))
        .bind(write_time(&delegation.requested_at))
        .bind(write_optional_time(delegation.completed_at.as_ref()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Attach the child run that carries a delegation out, and mark it running.
    ///
    /// Only a delegation still `requested` takes a run. One that already has a
    /// run, or has ended, is refused rather than repointed, so the edge a trace
    /// shows is the edge that was first recorded.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent or no longer `requested`, and
    /// [`DbError::Sql`] if the run does not exist or another delegation
    /// already claims it.
    pub async fn attach_run(&self, id: DelegationId, run_id: TaskRunId) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE delegations SET child_run_id = ?2, state = 'running'
              WHERE id = ?1 AND state = 'requested'",
        )
        .bind(id.to_string())
        .bind(run_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "requested delegation",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Record how a delegation ended.
    ///
    /// The ending is final: a delegation already in a terminal state is not
    /// rewritten, so a late cancellation cannot turn a recorded completion
    /// into something else, nor the reverse. A delegation refused before its
    /// run was prepared is ended from `requested` with no steps spent.
    ///
    /// # Errors
    ///
    /// [`DbError::Invalid`] if `state` is not terminal, and
    /// [`DbError::NotFound`] if the delegation is absent or has already ended.
    pub async fn complete(
        &self,
        id: DelegationId,
        state: DelegationState,
        steps_spent: u32,
    ) -> Result<(), DbError> {
        if !state.is_terminal() {
            return Err(DbError::Invalid {
                entity: "delegation state",
                value: state.as_str().to_owned(),
                reason: "a delegation can only be completed into a terminal state".to_owned(),
            });
        }

        let affected = sqlx::query(
            "UPDATE delegations SET state = ?2, steps_spent = ?3, completed_at = ?4
              WHERE id = ?1 AND state IN ('requested', 'running')",
        )
        .bind(id.to_string())
        .bind(state.as_str())
        .bind(i64::from(steps_spent))
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "unfinished delegation",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Fetch a delegation.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: DelegationId) -> Result<Delegation, DbError> {
        self.find(id).await?.ok_or(DbError::NotFound {
            entity: "delegation",
            id: id.to_string(),
        })
    }

    /// Fetch a delegation, or `None`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(&self, id: DelegationId) -> Result<Option<Delegation>, DbError> {
        let row = sqlx::query("SELECT * FROM delegations WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// The delegations a run made, in the order it made them.
    ///
    /// Ties on the timestamp — one call fanning out to several agents within
    /// the same microsecond — fall back to the order the rows were written.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_parent_run(
        &self,
        parent_run_id: TaskRunId,
    ) -> Result<Vec<Delegation>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM delegations WHERE parent_run_id = ?1 ORDER BY requested_at, rowid",
        )
        .bind(parent_run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// The delegation that started a run, or `None` for a run nobody delegated.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find_by_child_run(
        &self,
        child_run_id: TaskRunId,
    ) -> Result<Option<Delegation>, DbError> {
        let row = sqlx::query("SELECT * FROM delegations WHERE child_run_id = ?1")
            .bind(child_run_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// The delegation a task was created for, or `None` for a task nobody
    /// delegated.
    ///
    /// The runtime asks before it starts a task as a run of its own: a
    /// delegated task is carried out only through its delegation, under every
    /// policy above it, never on its own.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find_by_child_task(
        &self,
        child_task_id: TaskId,
    ) -> Result<Option<Delegation>, DbError> {
        let row = sqlx::query(
            "SELECT * FROM delegations WHERE child_task_id = ?1 ORDER BY requested_at, rowid \
             LIMIT 1",
        )
        .bind(child_task_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// The delegations a run made that have not ended, in the order it made
    /// them.
    ///
    /// For closing the edges of a run that will never conclude them: one
    /// still `requested` has no run for anything to find it by.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_open_for_parent_run(
        &self,
        parent_run_id: TaskRunId,
    ) -> Result<Vec<Delegation>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM delegations WHERE parent_run_id = ?1
                AND state IN ('requested', 'running')
              ORDER BY requested_at, rowid",
        )
        .bind(parent_run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Every delegation above a run: the one that started it first, the one
    /// that started the root's child last. Empty for a run nobody delegated.
    ///
    /// Stops after [`MAX_ANCESTRY_HOPS`] edges whatever the rows say, so a
    /// corrupt chain is returned truncated rather than walked for ever.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn ancestry(&self, run_id: TaskRunId) -> Result<Vec<Delegation>, DbError> {
        let mut chain = Vec::new();
        let mut current = run_id;
        while chain.len() < MAX_ANCESTRY_HOPS {
            let Some(edge) = self.find_by_child_run(current).await? else {
                break;
            };
            current = edge.parent_run_id;
            chain.push(edge);
        }
        Ok(chain)
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Delegation, DbError> {
    let depth: i64 = row.try_get("depth")?;
    let steps_spent: i64 = row.try_get("steps_spent")?;
    Ok(Delegation {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        parent_run_id: read_id(
            TABLE,
            "parent_run_id",
            row.try_get::<String, _>("parent_run_id")?.as_str(),
        )?,
        parent_agent_id: read_id(
            TABLE,
            "parent_agent_id",
            row.try_get::<String, _>("parent_agent_id")?.as_str(),
        )?,
        child_agent_id: read_id(
            TABLE,
            "child_agent_id",
            row.try_get::<String, _>("child_agent_id")?.as_str(),
        )?,
        child_task_id: read_id(
            TABLE,
            "child_task_id",
            row.try_get::<String, _>("child_task_id")?.as_str(),
        )?,
        child_run_id: read_optional_id(TABLE, "child_run_id", row.try_get("child_run_id")?)?,
        objective: row.try_get("objective")?,
        depth: u8::try_from(depth)
            .map_err(|error| DbError::corrupt(TABLE, "depth", depth.to_string(), error))?,
        state: read_enum(TABLE, "state", row.try_get::<String, _>("state")?.as_str())?,
        steps_spent: u32::try_from(steps_spent).map_err(|error| {
            DbError::corrupt(TABLE, "steps_spent", steps_spent.to_string(), error)
        })?,
        requested_at: read_time(
            TABLE,
            "requested_at",
            row.try_get::<String, _>("requested_at")?.as_str(),
        )?,
        completed_at: read_optional_time(TABLE, "completed_at", row.try_get("completed_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    use agentos_core::agent::Agent;
    use agentos_core::task::{Task, TaskRun};

    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    /// A run of a fresh task for `agent`, written.
    async fn run_for(db: &Database, agent: &Agent, parent: Option<TaskId>) -> TaskRun {
        let mut task = Task::new(agent.id, format!("work for {}", agent.name));
        if let Some(parent) = parent {
            task = task.with_parent(parent);
        }
        db.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        db.runs().insert(&run).await.unwrap();
        run
    }

    async fn agent(db: &Database, name: &str) -> Agent {
        let agent = sample_agent(name);
        db.agents().insert(&agent).await.unwrap();
        agent
    }

    /// A delegation from `parent`'s run to a new task of `child`, recorded
    /// and attached to a run of that task.
    async fn delegate(
        db: &Database,
        parent: &Agent,
        parent_run: &TaskRun,
        child: &Agent,
        depth: u8,
    ) -> (Delegation, TaskRun) {
        let child_run = run_for(db, child, Some(parent_run.task_id)).await;
        let delegation = Delegation::requested(
            parent_run.id,
            parent.id,
            child.id,
            child_run.task_id,
            format!("help {}", parent.name),
            depth,
        );
        db.delegations().insert(&delegation).await.unwrap();
        db.delegations()
            .attach_run(delegation.id, child_run.id)
            .await
            .unwrap();
        let attached = db.delegations().get(delegation.id).await.unwrap();
        (attached, child_run)
    }

    #[tokio::test]
    async fn round_trips_a_delegation_recorded_before_its_run() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let child_task =
            Task::new(worker.id, "Summarise the incident").with_parent(parent_run.task_id);
        db.tasks().insert(&child_task).await.unwrap();

        let written = Delegation::requested(
            parent_run.id,
            supervisor.id,
            worker.id,
            child_task.id,
            "Summarise the incident",
            1,
        );
        db.delegations().insert(&written).await.unwrap();

        let loaded = db.delegations().get(written.id).await.unwrap();
        assert_eq!(loaded, written);
        assert_eq!(loaded.child_run_id, None);
        assert!(
            db.delegations()
                .find(DelegationId::new())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_delegation_naming_a_run_that_does_not_exist_is_not_recorded() {
        // The broker writes the row before it starts anything and stops if the
        // write fails, so this refusal is what keeps an unrecordable delegation
        // from being performed.
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let real = run_for(&db, &worker, None).await;
        let orphan = Delegation::requested(
            TaskRunId::new(),
            supervisor.id,
            worker.id,
            real.task_id,
            "o",
            1,
        );
        let error = db.delegations().insert(&orphan).await.unwrap_err();
        assert!(matches!(error, DbError::Sql(_)), "{error:?}");
        assert!(db.delegations().find(orphan.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn attaching_then_completing_leaves_one_terminal_row() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (delegation, child_run) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;
        assert_eq!(delegation.state, DelegationState::Running);
        assert_eq!(delegation.child_run_id, Some(child_run.id));

        db.delegations()
            .complete(delegation.id, DelegationState::Completed, 3)
            .await
            .unwrap();
        let rows = db
            .delegations()
            .list_for_parent_run(parent_run.id)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, DelegationState::Completed);
        assert_eq!(rows[0].steps_spent, 3);
        assert!(rows[0].completed_at.is_some());

        // The ending is final, and an ended delegation takes no new run.
        let late = db
            .delegations()
            .complete(delegation.id, DelegationState::Cancelled, 0)
            .await
            .unwrap_err();
        assert!(matches!(late, DbError::NotFound { .. }), "{late:?}");
        let other_run = run_for(&db, &worker, None).await;
        let repoint = db
            .delegations()
            .attach_run(delegation.id, other_run.id)
            .await
            .unwrap_err();
        assert!(matches!(repoint, DbError::NotFound { .. }), "{repoint:?}");
        let reread = db.delegations().get(delegation.id).await.unwrap();
        assert_eq!(reread.state, DelegationState::Completed);
        assert_eq!(reread.child_run_id, Some(child_run.id));
    }

    #[tokio::test]
    async fn a_refusal_ends_a_delegation_that_never_had_a_run() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let child_task = Task::new(worker.id, "o");
        db.tasks().insert(&child_task).await.unwrap();
        let delegation = Delegation::requested(
            parent_run.id,
            supervisor.id,
            worker.id,
            child_task.id,
            "o",
            1,
        );
        db.delegations().insert(&delegation).await.unwrap();

        db.delegations()
            .complete(delegation.id, DelegationState::Refused, 0)
            .await
            .unwrap();
        let loaded = db.delegations().get(delegation.id).await.unwrap();
        assert_eq!(loaded.state, DelegationState::Refused);
        assert_eq!(loaded.child_run_id, None);
    }

    #[tokio::test]
    async fn completing_into_a_state_that_is_not_an_ending_is_refused() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (delegation, _) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;

        for state in [DelegationState::Requested, DelegationState::Running] {
            let error = db
                .delegations()
                .complete(delegation.id, state, 1)
                .await
                .unwrap_err();
            assert!(matches!(error, DbError::Invalid { .. }), "{error:?}");
        }
        assert_eq!(
            db.delegations().get(delegation.id).await.unwrap().state,
            DelegationState::Running
        );
    }

    #[tokio::test]
    async fn a_run_is_started_by_at_most_one_delegation() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (first, child_run) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;

        let second = Delegation::requested(
            parent_run.id,
            supervisor.id,
            worker.id,
            first.child_task_id,
            "again",
            1,
        );
        db.delegations().insert(&second).await.unwrap();
        let error = db
            .delegations()
            .attach_run(second.id, child_run.id)
            .await
            .unwrap_err();
        assert!(matches!(error, DbError::Sql(_)), "{error:?}");
        assert_eq!(
            db.delegations()
                .find_by_child_run(child_run.id)
                .await
                .unwrap()
                .map(|edge| edge.id),
            Some(first.id)
        );
    }

    #[tokio::test]
    async fn children_are_listed_in_the_order_they_were_asked_for() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let workers = [
            agent(&db, "first").await,
            agent(&db, "second").await,
            agent(&db, "third").await,
        ];
        let parent_run = run_for(&db, &supervisor, None).await;
        for worker in &workers {
            delegate(&db, &supervisor, &parent_run, worker, 1).await;
        }

        let children = db
            .delegations()
            .list_for_parent_run(parent_run.id)
            .await
            .unwrap();
        let asked: Vec<_> = children.iter().map(|edge| edge.child_agent_id).collect();
        assert_eq!(asked, workers.iter().map(|w| w.id).collect::<Vec<_>>());
        assert!(
            db.delegations()
                .list_for_parent_run(children[0].child_run_id.unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn ancestry_walks_up_to_the_root_and_ends_there() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let researcher = agent(&db, "researcher").await;
        let reader = agent(&db, "reader").await;
        let root = run_for(&db, &supervisor, None).await;
        let (upper, middle) = delegate(&db, &supervisor, &root, &researcher, 1).await;
        let (lower, leaf) = delegate(&db, &researcher, &middle, &reader, 2).await;

        let chain = db.delegations().ancestry(leaf.id).await.unwrap();
        assert_eq!(
            chain.iter().map(|edge| edge.id).collect::<Vec<_>>(),
            vec![lower.id, upper.id]
        );
        assert_eq!(chain.last().unwrap().parent_run_id, root.id);
        assert!(db.delegations().ancestry(root.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_corrupt_chain_cannot_hold_the_reader_for_ever() {
        // Two rows naming each other's runs: nothing the runtime writes, but a
        // database edited by hand can hold it.
        let db = Database::in_memory().await.unwrap();
        let a = agent(&db, "a").await;
        let b = agent(&db, "b").await;
        let a_run = run_for(&db, &a, None).await;
        let b_run = run_for(&db, &b, None).await;
        for (parent, parent_run, child, child_run) in
            [(&a, &a_run, &b, &b_run), (&b, &b_run, &a, &a_run)]
        {
            let edge = Delegation::requested(
                parent_run.id,
                parent.id,
                child.id,
                child_run.task_id,
                "o",
                1,
            );
            db.delegations().insert(&edge).await.unwrap();
            db.delegations()
                .attach_run(edge.id, child_run.id)
                .await
                .unwrap();
        }

        let chain = db.delegations().ancestry(a_run.id).await.unwrap();
        assert_eq!(chain.len(), MAX_ANCESTRY_HOPS);
    }

    #[tokio::test]
    async fn a_delegated_task_is_found_by_the_delegation_that_created_it() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (delegation, child_run) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;

        let found = db
            .delegations()
            .find_by_child_task(child_run.task_id)
            .await
            .unwrap();
        assert_eq!(found.map(|edge| edge.id), Some(delegation.id));
        assert!(
            db.delegations()
                .find_by_child_task(parent_run.task_id)
                .await
                .unwrap()
                .is_none(),
            "a task a person started was created by no delegation"
        );
    }

    #[tokio::test]
    async fn only_delegations_that_have_not_ended_are_open() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (running, _) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;
        let (ended, _) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;
        db.delegations()
            .complete(ended.id, DelegationState::Completed, 1)
            .await
            .unwrap();
        // Recorded and never given a run: what a process that died between
        // the two writes leaves behind.
        let unstarted_task = Task::new(worker.id, "o").with_parent(parent_run.task_id);
        db.tasks().insert(&unstarted_task).await.unwrap();
        let requested = Delegation::requested(
            parent_run.id,
            supervisor.id,
            worker.id,
            unstarted_task.id,
            "o",
            1,
        );
        db.delegations().insert(&requested).await.unwrap();

        let open = db
            .delegations()
            .list_open_for_parent_run(parent_run.id)
            .await
            .unwrap();
        assert_eq!(
            open.iter().map(|edge| edge.id).collect::<Vec<_>>(),
            vec![running.id, requested.id]
        );
    }

    #[tokio::test]
    async fn deleting_an_agent_removes_the_delegations_it_was_party_to() {
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let (delegation, _) = delegate(&db, &supervisor, &parent_run, &worker, 1).await;

        db.agents().delete(worker.id).await.unwrap();
        assert!(
            db.delegations()
                .find(delegation.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(db.runs().find(parent_run.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_depth_of_zero_is_not_a_delegation() {
        // A root run sits at depth zero and has no row; a row claiming depth
        // zero would make a child look like a root.
        let db = Database::in_memory().await.unwrap();
        let supervisor = agent(&db, "supervisor").await;
        let worker = agent(&db, "worker").await;
        let parent_run = run_for(&db, &supervisor, None).await;
        let child_task = Task::new(worker.id, "o");
        db.tasks().insert(&child_task).await.unwrap();
        let flat = Delegation::requested(
            parent_run.id,
            supervisor.id,
            worker.id,
            child_task.id,
            "o",
            0,
        );
        assert!(db.delegations().insert(&flat).await.is_err());
    }
}
