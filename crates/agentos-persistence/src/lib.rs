//! SQLite persistence for AgentOS.
//!
//! One embedded database holds everything: agents, their policies, tasks, runs,
//! execution traces, approvals, memory and the audit log. It lives on the user's
//! machine and nothing here talks to a network.
//!
//! Repositories are thin: they map rows to domain types and back, and contain no
//! business logic. Anything that decides *what* to write lives in the runtime.
//!
//! Queries use runtime-checked `sqlx::query` rather than the compile-time
//! `query!` macros, so building the project never requires a live `DATABASE_URL`.
//! Correctness comes from the tests in each repository instead.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod convert;
pub mod error;

pub mod agents;
pub mod approvals;
pub mod audit_sink;
pub mod executions;
pub mod memories;
pub mod runs;
pub mod settings;
pub mod steps;
pub mod tasks;

use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use agents::AgentRepository;
pub use approvals::ApprovalRepository;
pub use audit_sink::SqliteAuditSink;
pub use error::DbError;
pub use executions::{ExecutionRepository, ToolExecutionRecord};
pub use memories::MemoryRepository;
pub use runs::RunRepository;
pub use settings::SettingsRepository;
pub use steps::StepRepository;
pub use tasks::TaskRepository;

/// Embedded schema migrations.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// A connection pool with the AgentOS schema applied.
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Open (creating if needed) a database file and apply migrations.
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] if the file cannot be opened or migration fails.
    pub async fn open(path: &Path) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            // WAL lets the UI read while a run writes.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            // Off by default in SQLite; the schema depends on it for cascades.
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(10));

        Self::from_options(options, 8).await
    }

    /// Open a private in-memory database. Each call gets its own.
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] if migration fails.
    pub async fn in_memory() -> Result<Self, DbError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        // A single connection: with `:memory:` every new connection would get a
        // fresh, empty database.
        Self::from_options(options, 1).await
    }

    async fn from_options(
        options: SqliteConnectOptions,
        max_connections: u32,
    ) -> Result<Self, DbError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// The underlying pool, for repositories and tests.
    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Agents and their policies.
    #[must_use]
    pub fn agents(&self) -> AgentRepository {
        AgentRepository::new(self.pool.clone())
    }

    /// Tasks.
    #[must_use]
    pub fn tasks(&self) -> TaskRepository {
        TaskRepository::new(self.pool.clone())
    }

    /// Task runs.
    #[must_use]
    pub fn runs(&self) -> RunRepository {
        RunRepository::new(self.pool.clone())
    }

    /// Execution trace steps.
    #[must_use]
    pub fn steps(&self) -> StepRepository {
        StepRepository::new(self.pool.clone())
    }

    /// Tool executions.
    #[must_use]
    pub fn executions(&self) -> ExecutionRepository {
        ExecutionRepository::new(self.pool.clone())
    }

    /// Approval requests.
    #[must_use]
    pub fn approvals(&self) -> ApprovalRepository {
        ApprovalRepository::new(self.pool.clone())
    }

    /// Agent memory.
    #[must_use]
    pub fn memories(&self) -> MemoryRepository {
        MemoryRepository::new(self.pool.clone())
    }

    /// Key/value settings.
    #[must_use]
    pub fn settings(&self) -> SettingsRepository {
        SettingsRepository::new(self.pool.clone())
    }

    /// An audit sink backed by this database.
    #[must_use]
    pub fn audit_sink(&self) -> SqliteAuditSink {
        SqliteAuditSink::new(self.pool.clone())
    }

    /// Close the pool, waiting for in-flight statements.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_databases_are_migrated_and_isolated() {
        let a = Database::in_memory().await.unwrap();
        let b = Database::in_memory().await.unwrap();

        let agent = agents::tests::sample_agent("only-in-a");
        a.agents().insert(&agent).await.unwrap();

        assert_eq!(a.agents().list().await.unwrap().len(), 1);
        assert_eq!(b.agents().list().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_file_database_persists_across_opens() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("agentos.db");

        let agent = agents::tests::sample_agent("persisted");
        {
            let db = Database::open(&path).await.unwrap();
            db.agents().insert(&agent).await.unwrap();
            db.close().await;
        }

        let db = Database::open(&path).await.unwrap();
        assert_eq!(db.agents().get(agent.id).await.unwrap().name, "persisted");
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        // Without `foreign_keys = ON`, SQLite silently accepts orphan rows and
        // the cascade deletes in the schema would never fire.
        let db = Database::in_memory().await.unwrap();
        let orphan = agentos_core::task::Task::new(agentos_core::ids::AgentId::new(), "no agent");
        let err = db.tasks().insert(&orphan).await.unwrap_err();
        assert!(matches!(err, DbError::Sql(_)), "{err:?}");
    }
}
