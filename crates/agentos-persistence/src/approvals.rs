//! Approval requests.
//!
//! Approvals are persisted rather than held in memory so a pending request
//! survives a crash or restart, and so the audit trail can show what a human was
//! shown at the moment they decided.

use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::ids::{ApprovalId, TaskRunId};
use agentos_core::permission::Capability;
use agentos_core::risk::RiskLevel;
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_enum, read_id, read_json, read_optional_time, read_time, read_unit_enum, write_json,
    write_time,
};
use crate::error::DbError;

const TABLE: &str = "approvals";

/// Reads and writes approval requests.
#[derive(Debug, Clone)]
pub struct ApprovalRepository {
    pool: SqlitePool,
}

impl ApprovalRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Raise an approval request.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn insert(&self, request: &ApprovalRequest) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO approvals (id, agent_id, agent_name, task_id, run_id, tool, arguments,
                                    capability, risk, reason, explanation, affected_resources,
                                    tainted, status, requested_at, decided_at, decision_note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )
        .bind(request.id.to_string())
        .bind(request.agent_id.to_string())
        .bind(&request.agent_name)
        .bind(request.task_id.to_string())
        .bind(request.run_id.to_string())
        .bind(&request.tool)
        .bind(write_json("arguments", &request.arguments)?)
        .bind(write_json("capability", &request.capability)?)
        .bind(request.risk.as_str())
        .bind(&request.reason)
        .bind(&request.explanation)
        .bind(write_json(
            "affected_resources",
            &request.affected_resources,
        )?)
        .bind(i64::from(request.tainted))
        .bind(request.status.as_str())
        .bind(write_time(&request.requested_at))
        .bind(crate::convert::write_optional_time(
            request.decided_at.as_ref(),
        ))
        .bind(request.decision_note.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch a request.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: ApprovalId) -> Result<ApprovalRequest, DbError> {
        let row = sqlx::query("SELECT * FROM approvals WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row))
            .transpose()?
            .ok_or(DbError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })
    }

    /// Requests still waiting on a human, oldest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_pending(&self) -> Result<Vec<ApprovalRequest>, DbError> {
        let rows =
            sqlx::query("SELECT * FROM approvals WHERE status = 'pending' ORDER BY requested_at")
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Requests raised during a run, oldest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_run(&self, run_id: TaskRunId) -> Result<Vec<ApprovalRequest>, DbError> {
        let rows = sqlx::query("SELECT * FROM approvals WHERE run_id = ?1 ORDER BY requested_at")
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Record a decision.
    ///
    /// Only a `pending` request can be decided; deciding an already-decided
    /// request is rejected rather than silently overwriting the first answer.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent or no longer pending.
    pub async fn decide(
        &self,
        id: ApprovalId,
        status: ApprovalStatus,
        note: Option<&str>,
    ) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE approvals
                SET status = ?2, decided_at = ?3, decision_note = ?4
              WHERE id = ?1 AND status = 'pending'",
        )
        .bind(id.to_string())
        .bind(status.as_str())
        .bind(write_time(&agentos_core::now()))
        .bind(note)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "pending approval",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Cancel every pending request belonging to a run.
    ///
    /// Called when a run is cancelled, so the approvals queue does not fill with
    /// requests nobody can usefully answer.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn cancel_pending_for_run(&self, run_id: TaskRunId) -> Result<u64, DbError> {
        let affected = sqlx::query(
            "UPDATE approvals SET status = 'cancelled', decided_at = ?2
              WHERE run_id = ?1 AND status = 'pending'",
        )
        .bind(run_id.to_string())
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(affected)
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<ApprovalRequest, DbError> {
    Ok(ApprovalRequest {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        agent_id: read_id(
            TABLE,
            "agent_id",
            row.try_get::<String, _>("agent_id")?.as_str(),
        )?,
        agent_name: row.try_get("agent_name")?,
        task_id: read_id(
            TABLE,
            "task_id",
            row.try_get::<String, _>("task_id")?.as_str(),
        )?,
        run_id: read_id(
            TABLE,
            "run_id",
            row.try_get::<String, _>("run_id")?.as_str(),
        )?,
        tool: row.try_get("tool")?,
        arguments: read_json(
            TABLE,
            "arguments",
            row.try_get::<String, _>("arguments")?.as_str(),
        )?,
        capability: read_json::<Capability>(
            TABLE,
            "capability",
            row.try_get::<String, _>("capability")?.as_str(),
        )?,
        risk: read_enum::<RiskLevel>(TABLE, "risk", row.try_get::<String, _>("risk")?.as_str())?,
        reason: row.try_get("reason")?,
        explanation: row.try_get("explanation")?,
        affected_resources: read_json(
            TABLE,
            "affected_resources",
            row.try_get::<String, _>("affected_resources")?.as_str(),
        )?,
        tainted: row.try_get::<i64, _>("tainted")? != 0,
        status: read_unit_enum::<ApprovalStatus>(
            TABLE,
            "status",
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        requested_at: read_time(
            TABLE,
            "requested_at",
            row.try_get::<String, _>("requested_at")?.as_str(),
        )?,
        decided_at: read_optional_time(TABLE, "decided_at", row.try_get("decided_at")?)?,
        decision_note: row.try_get("decision_note")?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use agentos_core::ids::{AgentId, TaskId};
    use agentos_core::task::{Task, TaskRun};

    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    pub(crate) struct Context {
        pub db: Database,
        pub agent_id: AgentId,
        pub task_id: TaskId,
        pub run_id: TaskRunId,
    }

    pub(crate) async fn seeded() -> Context {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("approver");
        db.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "o");
        db.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        db.runs().insert(&run).await.unwrap();
        Context {
            db,
            agent_id: agent.id,
            task_id: task.id,
            run_id: run.id,
        }
    }

    fn request(ctx: &Context, tool: &str) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: ctx.agent_id,
            agent_name: "approver".to_owned(),
            task_id: ctx.task_id,
            run_id: ctx.run_id,
            tool: tool.to_owned(),
            arguments: serde_json::json!({"to": "customer@example.com"}),
            capability: Capability::new("email", "send"),
            risk: RiskLevel::High,
            reason: "policy rule `email.send` requires approval".to_owned(),
            explanation: "Send an order update to customer@example.com.".to_owned(),
            affected_resources: vec!["customer@example.com".to_owned()],
            tainted: true,
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        }
    }

    #[tokio::test]
    async fn round_trips_a_request() {
        let ctx = seeded().await;
        let written = request(&ctx, "email.send");
        ctx.db.approvals().insert(&written).await.unwrap();

        let loaded = ctx.db.approvals().get(written.id).await.unwrap();
        assert_eq!(loaded, written);
        assert!(loaded.tainted);
        assert_eq!(loaded.capability, Capability::new("email", "send"));
    }

    #[tokio::test]
    async fn pending_requests_are_listed_oldest_first() {
        let ctx = seeded().await;
        let first = request(&ctx, "a");
        let second = request(&ctx, "b");
        ctx.db.approvals().insert(&first).await.unwrap();
        ctx.db.approvals().insert(&second).await.unwrap();

        let pending = ctx.db.approvals().list_pending().await.unwrap();
        assert_eq!(pending.len(), 2);

        ctx.db
            .approvals()
            .decide(first.id, ApprovalStatus::Approved, Some("looks right"))
            .await
            .unwrap();
        assert_eq!(ctx.db.approvals().list_pending().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_decision_is_final() {
        let ctx = seeded().await;
        let written = request(&ctx, "email.send");
        ctx.db.approvals().insert(&written).await.unwrap();

        ctx.db
            .approvals()
            .decide(written.id, ApprovalStatus::Denied, Some("wrong recipient"))
            .await
            .unwrap();

        let loaded = ctx.db.approvals().get(written.id).await.unwrap();
        assert_eq!(loaded.status, ApprovalStatus::Denied);
        assert_eq!(loaded.decision_note.as_deref(), Some("wrong recipient"));
        assert!(loaded.decided_at.is_some());

        // A second decision must not overwrite the first.
        let err = ctx
            .db
            .approvals()
            .decide(written.id, ApprovalStatus::Approved, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::NotFound { .. }));
        assert_eq!(
            ctx.db.approvals().get(written.id).await.unwrap().status,
            ApprovalStatus::Denied
        );
    }

    #[tokio::test]
    async fn cancelling_a_run_clears_its_pending_approvals() {
        let ctx = seeded().await;
        let pending = request(&ctx, "a");
        let decided = request(&ctx, "b");
        ctx.db.approvals().insert(&pending).await.unwrap();
        ctx.db.approvals().insert(&decided).await.unwrap();
        ctx.db
            .approvals()
            .decide(decided.id, ApprovalStatus::Approved, None)
            .await
            .unwrap();

        let cancelled = ctx
            .db
            .approvals()
            .cancel_pending_for_run(ctx.run_id)
            .await
            .unwrap();
        assert_eq!(cancelled, 1);
        assert_eq!(
            ctx.db.approvals().get(pending.id).await.unwrap().status,
            ApprovalStatus::Cancelled
        );
        // The already-decided one is untouched.
        assert_eq!(
            ctx.db.approvals().get(decided.id).await.unwrap().status,
            ApprovalStatus::Approved
        );
    }

    #[tokio::test]
    async fn requests_are_listed_per_run() {
        let ctx = seeded().await;
        ctx.db
            .approvals()
            .insert(&request(&ctx, "a"))
            .await
            .unwrap();
        assert_eq!(
            ctx.db
                .approvals()
                .list_for_run(ctx.run_id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            ctx.db
                .approvals()
                .list_for_run(TaskRunId::new())
                .await
                .unwrap()
                .len(),
            0
        );
    }
}
