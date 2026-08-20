//! Key/value settings.
//!
//! Non-secret configuration only. API keys live in the OS keychain via
//! `agentos-secrets`; nothing credential-shaped belongs in this table.

use sqlx::{Row, SqlitePool};

use crate::convert::write_time;
use crate::error::DbError;

/// Reads and writes settings.
#[derive(Debug, Clone)]
pub struct SettingsRepository {
    pool: SqlitePool,
}

/// Setting keys the runtime itself uses.
pub mod keys {
    /// Default provider id for new agents.
    pub const DEFAULT_PROVIDER: &str = "default.provider";
    /// Default model id for new agents.
    pub const DEFAULT_MODEL: &str = "default.model";
    /// Base URL for the OpenAI-compatible provider.
    pub const OPENAI_BASE_URL: &str = "provider.openai.base_url";
    /// Path to the Chromium executable the browser tools should drive.
    pub const BROWSER_EXECUTABLE: &str = "browser.executable";
}

impl SettingsRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Read a setting.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn get(&self, key: &str) -> Result<Option<String>, DbError> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| row.try_get("value"))
            .transpose()
            .map_err(Into::into)
    }

    /// Read a setting, or a default.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn get_or(&self, key: &str, fallback: &str) -> Result<String, DbError> {
        Ok(self.get(key).await?.unwrap_or_else(|| fallback.to_owned()))
    }

    /// Write a setting.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn set(&self, key: &str, value: &str) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove a setting. Removing something absent is not an error.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn delete(&self, key: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM settings WHERE key = ?1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every setting, for the settings screen and `agentos doctor`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn all(&self) -> Result<Vec<(String, String)>, DbError> {
        let rows = sqlx::query("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| Ok((row.try_get("key")?, row.try_get("value")?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[tokio::test]
    async fn settings_round_trip_and_overwrite() {
        let db = Database::in_memory().await.unwrap();
        assert!(
            db.settings()
                .get(keys::DEFAULT_MODEL)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.settings()
                .get_or(keys::DEFAULT_MODEL, "fallback")
                .await
                .unwrap(),
            "fallback"
        );

        db.settings()
            .set(keys::DEFAULT_MODEL, "claude-opus-5")
            .await
            .unwrap();
        assert_eq!(
            db.settings()
                .get(keys::DEFAULT_MODEL)
                .await
                .unwrap()
                .as_deref(),
            Some("claude-opus-5")
        );

        db.settings()
            .set(keys::DEFAULT_MODEL, "revised")
            .await
            .unwrap();
        assert_eq!(
            db.settings()
                .get(keys::DEFAULT_MODEL)
                .await
                .unwrap()
                .as_deref(),
            Some("revised")
        );
    }

    #[tokio::test]
    async fn settings_can_be_listed_and_deleted() {
        let db = Database::in_memory().await.unwrap();
        db.settings().set("b", "2").await.unwrap();
        db.settings().set("a", "1").await.unwrap();

        assert_eq!(
            db.settings().all().await.unwrap(),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );

        db.settings().delete("a").await.unwrap();
        db.settings().delete("never-existed").await.unwrap();
        assert_eq!(db.settings().all().await.unwrap().len(), 1);
    }
}
