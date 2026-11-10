//! Tool executions: what was attempted, what was decided, what happened.
//!
//! This table is the answer to "what did the agent actually do, and who let it?".
//! Every row records the permission effect and the taint state at the moment of
//! the call, so a later reader does not have to reconstruct them from the policy
//! as it stands today.

use std::collections::BTreeMap;

use agentos_core::Timestamp;
use agentos_core::ids::{ApprovalId, TaskRunId, ToolExecutionId};
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolOutcome;
use chrono::{Days, Local, NaiveDate, NaiveTime};
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
    /// Calls on each day of the window, oldest first.
    ///
    /// One entry per day, days with no calls included, so it is exactly as
    /// long as [`UsageWindow::days`] and always sums to `calls`: both come
    /// from the same grouped rows. A day is a local calendar day, as
    /// [`UsageWindow`] explains.
    pub daily: Vec<u64>,
}

/// The whole calendar days tool usage is counted over: `days` of them, the
/// last one included.
///
/// A day is this machine's local calendar day, because that is the day an
/// operator reads a chart in: a call made at half past eleven at night belongs
/// to that evening, not to tomorrow because UTC has already turned over.
/// Executions are stored in UTC, and the query converts each to a local date
/// with SQLite's `localtime`, which reads the same zone database as
/// [`chrono::Local`]; so the days [`UsageWindow::ending_today`] chooses and
/// the days rows land in agree. A day on which the clocks change is still one
/// day, 23 or 25 hours long.
///
/// The window is whole days rather than a span of hours back from now, so
/// that the totals and the per-day counts cover the same calls: seven days is
/// today and the six before it, from the first of those days' midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageWindow {
    first: NaiveDate,
    last: NaiveDate,
}

impl UsageWindow {
    /// The `days` days that end with `last`.
    ///
    /// A window always holds the day it ends on, so zero days is taken as one.
    #[must_use]
    pub fn ending(last: NaiveDate, days: u32) -> Self {
        let back = Days::new(u64::from(days.max(1) - 1));
        Self {
            first: last.checked_sub_days(back).unwrap_or(NaiveDate::MIN),
            last,
        }
    }

    /// The `days` days that end with the local day `at` falls on.
    #[must_use]
    pub fn ending_on(at: &Timestamp, days: u32) -> Self {
        Self::ending(at.with_timezone(&Local).date_naive(), days)
    }

    /// The `days` days that end today, by this machine's clock.
    #[must_use]
    pub fn ending_today(days: u32) -> Self {
        Self::ending_on(&agentos_core::now(), days)
    }

    /// The oldest day in the window.
    #[must_use]
    pub const fn first(&self) -> NaiveDate {
        self.first
    }

    /// The newest day in the window.
    #[must_use]
    pub const fn last(&self) -> NaiveDate {
        self.last
    }

    /// How many days the window holds, both ends included.
    #[must_use]
    pub fn days(&self) -> usize {
        self.index_of(self.last).map_or(1, |index| index + 1)
    }

    /// Where `day` falls in the window, from 0 for the first day, if it does.
    fn index_of(&self, day: NaiveDate) -> Option<usize> {
        if day < self.first || day > self.last {
            return None;
        }
        usize::try_from((day - self.first).num_days()).ok()
    }

    /// UTC instants safely either side of the window, as stored text.
    ///
    /// No zone is a whole day from UTC, so a call on one of the window's local
    /// days was made after midnight UTC the day before the first, and before
    /// midnight UTC two days after the last. Comparing the stored text with
    /// these needs only the entry in `idx_executions_tool`, so a row outside
    /// them is passed over before its table row is read or its local date
    /// worked out; the exact test on the local date runs only on the rows
    /// inside.
    fn utc_bounds(&self) -> (String, String) {
        let midnight = |day: NaiveDate| write_time(&day.and_time(NaiveTime::MIN).and_utc());
        let before = self.first.pred_opt().unwrap_or(self.first);
        let after = self
            .last
            .checked_add_days(Days::new(2))
            .unwrap_or(self.last);
        (midnight(before), midnight(after))
    }
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
    /// What each tool has been doing over `window`, busiest first, with the
    /// calls on each of its days.
    ///
    /// One query grouped by tool and day rather than a row per call, folded
    /// here into one [`ToolUsage`] per tool: its totals are the sums of its
    /// days, so the daily counts cannot disagree with them. Tools with no
    /// calls in the window are not listed; tools with equal counts are in name
    /// order.
    ///
    /// The local date is taken from the timestamp cut to the whole second.
    /// Every timestamp in this table is written by the same fixed-width
    /// RFC3339 UTC formatter at microsecond precision, so the first nineteen
    /// characters are the UTC second, and cutting there keeps the date out of
    /// reach of how a given SQLite release treats fractions of a second: older
    /// ones rounded to the millisecond, which carried a call in the last
    /// half-millisecond before midnight into the next day. The same fixed
    /// width makes lexical order chronological, which is what lets the outer
    /// bounds of [`UsageWindow`] be compared as text.
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
    pub async fn usage_over(&self, window: UsageWindow) -> Result<Vec<ToolUsage>, DbError> {
        let (after, before) = window.utc_bounds();
        let rows = sqlx::query(
            "SELECT tool,
                    date(substr(started_at, 1, 19), 'localtime') AS day,
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
              WHERE started_at >= ?1 AND started_at < ?2
                AND date(substr(started_at, 1, 19), 'localtime') BETWEEN ?3 AND ?4
              GROUP BY tool, day",
        )
        .bind(after)
        .bind(before)
        .bind(window.first().to_string())
        .bind(window.last().to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut tools: BTreeMap<String, ToolUsage> = BTreeMap::new();
        for row in &rows {
            let count = |column: &str| -> Result<u64, DbError> {
                Ok(u64::try_from(row.try_get::<i64, _>(column)?).unwrap_or(0))
            };
            let day: String = row.try_get("day")?;
            // The query only returns days inside the window, so a miss here
            // is a date SQLite wrote that chrono cannot read back.
            let index = NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                .ok()
                .and_then(|date| window.index_of(date))
                .ok_or_else(|| {
                    DbError::corrupt(TABLE, "started_at", day.as_str(), "outside the window")
                })?;
            let last_used_at = read_time(
                TABLE,
                "started_at",
                row.try_get::<String, _>("last_used_at")?.as_str(),
            )?;
            let tool: String = row.try_get("tool")?;
            let usage = tools.entry(tool.clone()).or_insert_with(|| ToolUsage {
                tool,
                calls: 0,
                executed: 0,
                denied: 0,
                approval_denied: 0,
                failed: 0,
                under_taint: 0,
                approvals: 0,
                total_duration_ms: 0,
                last_used_at,
                daily: vec![0; window.days()],
            });
            let calls = count("calls")?;
            usage.calls += calls;
            usage.executed += count("executed")?;
            usage.denied += count("denied")?;
            usage.approval_denied += count("approval_denied")?;
            usage.failed += count("failed")?;
            usage.under_taint += count("under_taint")?;
            usage.approvals += count("approvals")?;
            usage.total_duration_ms += count("total_duration_ms")?;
            usage.last_used_at = usage.last_used_at.max(last_used_at);
            if let Some(slot) = usage.daily.get_mut(index) {
                *slot += calls;
            }
        }

        let mut usage: Vec<ToolUsage> = tools.into_values().collect();
        // Stable, so tools with equal counts stay in name order.
        usage.sort_by_key(|tool| std::cmp::Reverse(tool.calls));
        Ok(usage)
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
    use chrono::TimeZone;

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

    /// Today and yesterday: the calls a test has just made are in it even if
    /// midnight passed while it ran.
    fn today_and_yesterday() -> UsageWindow {
        UsageWindow::ending_today(2)
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    /// The instant a wall clock on this machine reads `hms.micro` on `day`.
    ///
    /// The earlier reading where the clocks go back and the time repeats; an
    /// hour later where they go forward and the time never happens, which
    /// none of the times these tests use do in a zone they are likely run in.
    fn local(day: NaiveDate, (h, m, s): (u32, u32, u32), micro: u32) -> Timestamp {
        let wall = day.and_hms_micro_opt(h, m, s, micro).unwrap();
        Local
            .from_local_datetime(&wall)
            .earliest()
            .or_else(|| {
                Local
                    .from_local_datetime(&(wall + chrono::Duration::hours(1)))
                    .earliest()
            })
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    async fn call_at(db: &Database, run_id: TaskRunId, tool: &str, at: Timestamp) {
        let mut call = record(run_id, tool, ToolOutcome::Success, Effect::Allow);
        call.started_at = at;
        db.executions().insert(&call).await.unwrap();
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
            .usage_over(today_and_yesterday())
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
        assert_eq!(t.daily.iter().sum::<u64>(), t.calls);
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
            .usage_over(today_and_yesterday())
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
            .usage_over(UsageWindow::ending_today(7))
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
    async fn the_window_runs_from_the_first_local_midnight_to_the_last() {
        let (db, run_id) = seeded().await;
        let window = UsageWindow::ending(date(2026, 7, 14), 7);
        assert_eq!(window.first(), date(2026, 7, 8));
        assert_eq!(window.days(), 7);

        let last_moment = local(date(2026, 7, 14), (23, 59, 59), 999_999);
        for at in [
            // A microsecond before the window and the moment after it.
            local(date(2026, 7, 7), (23, 59, 59), 999_999),
            local(date(2026, 7, 15), (0, 0, 0), 0),
            // Its first moment, two calls in the middle and its last moment.
            local(date(2026, 7, 8), (0, 0, 0), 0),
            local(date(2026, 7, 10), (12, 0, 0), 0),
            local(date(2026, 7, 10), (12, 0, 1), 0),
            last_moment,
        ] {
            call_at(&db, run_id, "t", at).await;
        }

        let usage = db.executions().usage_over(window).await.unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].daily, vec![1, 0, 2, 0, 0, 0, 1]);
        assert_eq!(usage[0].calls, 4);
        assert_eq!(usage[0].last_used_at, last_moment);
        // A call late in the evening ends a window on its own evening's day,
        // whatever date UTC has reached.
        assert_eq!(UsageWindow::ending_on(&last_moment, 7), window);
    }

    #[tokio::test]
    async fn quiet_days_are_zeros_and_each_tool_sums_to_its_calls() {
        let (db, run_id) = seeded().await;
        let window = UsageWindow::ending(date(2026, 7, 14), 5);
        call_at(&db, run_id, "busy", local(date(2026, 7, 10), (9, 0, 0), 0)).await;
        for minute in 0..3 {
            call_at(
                &db,
                run_id,
                "busy",
                local(date(2026, 7, 13), (9, minute, 0), 0),
            )
            .await;
        }
        let mut refused = record(run_id, "quiet", ToolOutcome::Denied, Effect::Deny);
        refused.started_at = local(date(2026, 7, 12), (18, 0, 0), 0);
        db.executions().insert(&refused).await.unwrap();
        call_at(
            &db,
            run_id,
            "quiet",
            local(date(2026, 7, 12), (18, 5, 0), 0),
        )
        .await;

        let usage = db.executions().usage_over(window).await.unwrap();
        let tools: Vec<&str> = usage.iter().map(|tool| tool.tool.as_str()).collect();
        assert_eq!(tools, ["busy", "quiet"], "busiest first");
        assert_eq!(usage[0].daily, vec![1, 0, 0, 3, 0]);
        assert_eq!(usage[1].daily, vec![0, 0, 2, 0, 0]);
        // The buckets are summed across days as the calls are.
        assert_eq!((usage[1].executed, usage[1].denied), (1, 1));
        assert_eq!(usage[1].total_duration_ms, 34);
        for tool in &usage {
            assert_eq!(tool.daily.len(), window.days());
            assert_eq!(tool.daily.iter().sum::<u64>(), tool.calls, "{}", tool.tool);
        }

        // A day with no calls from anything is a window with nothing in it.
        let empty = UsageWindow::ending(date(2026, 7, 11), 1);
        assert!(db.executions().usage_over(empty).await.unwrap().is_empty());
    }

    #[test]
    fn a_window_always_holds_the_day_it_ends_on() {
        let day = date(2026, 7, 14);
        assert_eq!(UsageWindow::ending(day, 0), UsageWindow::ending(day, 1));
        let one = UsageWindow::ending(day, 1);
        assert_eq!((one.first(), one.last(), one.days()), (day, day, 1));
        assert_eq!(UsageWindow::ending(day, 30).days(), 30);
    }

    #[tokio::test]
    async fn a_day_the_clocks_change_on_is_still_one_day() {
        // Late March and October into November hold the changes in Europe and
        // North America. On a machine whose zone has none they are ordinary
        // days, and the test still holds.
        let (db, run_id) = seeded().await;
        let changes = [
            date(2026, 3, 8),
            date(2026, 3, 29),
            date(2026, 10, 25),
            date(2026, 11, 1),
        ];
        for day in changes {
            call_at(&db, run_id, "t", local(day, (0, 30, 0), 0)).await;
            call_at(&db, run_id, "t", local(day, (23, 30, 0), 0)).await;
        }

        let window = UsageWindow::ending(date(2026, 11, 2), 250);
        let usage = db.executions().usage_over(window).await.unwrap();
        let t = &usage[0];
        assert_eq!(t.calls, 8);
        for day in changes {
            let index = window.index_of(day).unwrap();
            assert_eq!(t.daily[index], 2, "both of {day}'s calls on {day}");
        }
        assert_eq!(t.daily.iter().sum::<u64>(), t.calls);
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
