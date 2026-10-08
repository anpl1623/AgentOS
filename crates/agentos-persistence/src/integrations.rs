//! Bound integration accounts.
//!
//! A row says that the operator bound an account of an integration: which
//! integration, under what label, at which API host, and whether it may reach
//! the operator's private network. It does not hold the token. The token is a
//! network credential stored for the origin of `host` under `label`, and the
//! runtime writes it before the row and removes it after the row, so the only
//! inconsistency a crash can leave is a secret nothing points at.
//!
//! There is deliberately no update. The host decides where every request for
//! the account goes and the flag decides which addresses it may reach; both
//! are set when the operator binds the account, and changing either is an
//! unbind and a bind, each of them on the record.

use agentos_core::Timestamp;
use agentos_core::ids::IntegrationAccountId;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::convert::{read_id, read_optional_time, read_time, write_time};
use crate::error::DbError;

const TABLE: &str = "integration_accounts";

/// The longest an account label or an integration name may be.
pub const MAX_LABEL_LEN: usize = 32;

/// Whether `label` can name an account: 1 to [`MAX_LABEL_LEN`] lowercase ASCII
/// letters, digits and `-`.
///
/// The label is also the name of the account's credential and a segment of
/// the source every response is recorded under, `github:work@/repos/...`.
/// The alphabet is a subset of what a credential name may be, so every label
/// can be stored, and it has no `/`, `.`, `@` or glob character that would
/// move a boundary in any of those spellings. The schema holds the same rule.
#[must_use]
pub fn is_account_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// One bound account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationAccount {
    /// Identity.
    pub id: IntegrationAccountId,
    /// The integration, e.g. `github`.
    pub integration: String,
    /// The operator's name for the account, and the name of its credential.
    pub label: String,
    /// The API base URL every request for the account goes to, e.g.
    /// `https://api.github.com` or `https://ghe.example/api/v3`.
    pub host: String,
    /// Whether the operator allowed the account to reach private-network
    /// addresses, for a server on their own network.
    pub private_network: bool,
    /// What the operator says the token can do. A note: what an agent may do
    /// with the account is the policy's to say, whatever the token allows.
    pub scopes: Option<String>,
    /// When it was bound.
    pub created_at: Timestamp,
    /// When a call last acted as it.
    pub last_used_at: Option<Timestamp>,
}

impl IntegrationAccount {
    /// A new account, bound now and never used.
    #[must_use]
    pub fn new(
        integration: impl Into<String>,
        label: impl Into<String>,
        host: impl Into<String>,
        private_network: bool,
        scopes: Option<String>,
    ) -> Self {
        Self {
            id: IntegrationAccountId::new(),
            integration: integration.into(),
            label: label.into(),
            host: host.into(),
            private_network,
            scopes,
            created_at: agentos_core::now(),
            last_used_at: None,
        }
    }
}

/// Reads and writes bound integration accounts.
#[derive(Debug, Clone)]
pub struct IntegrationAccountsRepository {
    pool: SqlitePool,
}

impl IntegrationAccountsRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Every bound account, by integration and then oldest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list(&self) -> Result<Vec<IntegrationAccount>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM integration_accounts ORDER BY integration, created_at, label",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// The accounts bound for one integration, oldest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for(&self, integration: &str) -> Result<Vec<IntegrationAccount>, DbError> {
        let rows = sqlx::query(
            "SELECT * FROM integration_accounts WHERE integration = ?1
              ORDER BY created_at, label",
        )
        .bind(integration)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Fetch an account.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: IntegrationAccountId) -> Result<IntegrationAccount, DbError> {
        let row = sqlx::query("SELECT * FROM integration_accounts WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(hydrate)
            .transpose()?
            .ok_or(DbError::NotFound {
                entity: "integration account",
                id: id.to_string(),
            })
    }

    /// The account of `integration` labelled `label`, if one is bound.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(
        &self,
        integration: &str,
        label: &str,
    ) -> Result<Option<IntegrationAccount>, DbError> {
        let row =
            sqlx::query("SELECT * FROM integration_accounts WHERE integration = ?1 AND label = ?2")
                .bind(integration)
                .bind(label)
                .fetch_optional(&self.pool)
                .await?;
        row.as_ref().map(hydrate).transpose()
    }

    /// Record a bound account.
    ///
    /// The caller has already stored its credential: the row is written
    /// second, so that it never points at nothing.
    ///
    /// # Errors
    ///
    /// [`DbError::Invalid`] for a label or integration name outside
    /// [`is_account_label`]'s alphabet; [`DbError::Conflict`] if the label is
    /// taken for that integration; [`DbError::Sql`] otherwise.
    pub async fn insert(&self, account: &IntegrationAccount) -> Result<(), DbError> {
        for (field, value) in [
            ("integration", &account.integration),
            ("label", &account.label),
        ] {
            if !is_account_label(value) {
                return Err(DbError::Invalid {
                    entity: "integration account",
                    value: value.clone(),
                    reason: format!(
                        "the {field} must be 1 to {MAX_LABEL_LEN} lowercase letters, digits or `-`"
                    ),
                });
            }
        }
        let result = sqlx::query(
            "INSERT INTO integration_accounts (id, integration, label, host, private_network,
                                               scopes, created_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(account.id.to_string())
        .bind(&account.integration)
        .bind(&account.label)
        .bind(&account.host)
        .bind(i64::from(account.private_network))
        .bind(account.scopes.as_deref())
        .bind(write_time(&account.created_at))
        .bind(account.last_used_at.as_ref().map(write_time))
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(DbError::Conflict {
                    entity: "integration account",
                    value: format!("{}:{}", account.integration, account.label),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Delete an account, returning the row as it was.
    ///
    /// Only the row: the credential behind it is the caller's to remove, after
    /// this returns, and the row handed back is what names it. Deleting the
    /// row first means a crash between the two leaves a secret nothing points
    /// at, which `agentos integration list` cannot show but nothing can spend,
    /// rather than an account that fails on every call.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn delete(&self, id: IntegrationAccountId) -> Result<IntegrationAccount, DbError> {
        let row = sqlx::query("DELETE FROM integration_accounts WHERE id = ?1 RETURNING *")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(hydrate)
            .transpose()?
            .ok_or(DbError::NotFound {
                entity: "integration account",
                id: id.to_string(),
            })
    }

    /// Record that a call acted as the account just now.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent, which a call racing an unbind can see.
    pub async fn touch(&self, id: IntegrationAccountId) -> Result<(), DbError> {
        let affected =
            sqlx::query("UPDATE integration_accounts SET last_used_at = ?2 WHERE id = ?1")
                .bind(id.to_string())
                .bind(write_time(&agentos_core::now()))
                .execute(&self.pool)
                .await?
                .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "integration account",
                id: id.to_string(),
            });
        }
        Ok(())
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<IntegrationAccount, DbError> {
    let private_network: i64 = row.try_get("private_network")?;
    let private_network = match private_network {
        0 => false,
        1 => true,
        other => {
            return Err(DbError::corrupt(
                TABLE,
                "private_network",
                other.to_string(),
                "expected 0 or 1",
            ));
        }
    };
    Ok(IntegrationAccount {
        id: read_id(TABLE, "id", &row.try_get::<String, _>("id")?)?,
        integration: row.try_get("integration")?,
        label: row.try_get("label")?,
        host: row.try_get("host")?,
        private_network,
        scopes: row.try_get("scopes")?,
        created_at: read_time(
            TABLE,
            "created_at",
            &row.try_get::<String, _>("created_at")?,
        )?,
        last_used_at: read_optional_time(TABLE, "last_used_at", row.try_get("last_used_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn work() -> IntegrationAccount {
        IntegrationAccount::new(
            "github",
            "work",
            "https://api.github.com",
            false,
            Some("repo".into()),
        )
    }

    #[tokio::test]
    async fn an_account_round_trips_and_is_found_by_its_label() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        let account = work();
        repo.insert(&account).await.unwrap();

        assert_eq!(repo.get(account.id).await.unwrap(), account);
        assert_eq!(
            repo.find("github", "work").await.unwrap(),
            Some(account.clone())
        );
        assert_eq!(repo.find("github", "home").await.unwrap(), None);
        assert_eq!(repo.find("linear", "work").await.unwrap(), None);
        assert_eq!(repo.list().await.unwrap(), vec![account.clone()]);
        assert_eq!(repo.list_for("github").await.unwrap(), vec![account]);
        assert!(repo.list_for("linear").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_private_network_flag_is_stored_as_written() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        let enterprise =
            IntegrationAccount::new("github", "corp", "https://ghe.example/api/v3", true, None);
        repo.insert(&enterprise).await.unwrap();
        repo.insert(&work()).await.unwrap();

        let stored = repo.list_for("github").await.unwrap();
        let flags: Vec<(&str, bool)> = stored
            .iter()
            .map(|account| (account.label.as_str(), account.private_network))
            .collect();
        assert_eq!(flags, vec![("corp", true), ("work", false)]);
    }

    #[tokio::test]
    async fn a_label_is_bound_once_per_integration() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        repo.insert(&work()).await.unwrap();
        let error = repo.insert(&work()).await.unwrap_err();
        assert!(matches!(error, DbError::Conflict { .. }), "{error}");

        // The same label under another integration is another account.
        let other =
            IntegrationAccount::new("linear", "work", "https://api.linear.app", false, None);
        repo.insert(&other).await.unwrap();
        assert_eq!(repo.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_label_outside_the_alphabet_is_refused_by_the_repository_and_the_schema() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        for label in ["", "Work", "a/b", "a.b", "a@b", "a*", &"a".repeat(33)] {
            let mut account = work();
            account.label = label.to_owned();
            let error = repo.insert(&account).await.unwrap_err();
            assert!(matches!(error, DbError::Invalid { .. }), "{label}: {error}");
        }
        assert!(is_account_label("work-2"));
        assert!(is_account_label(&"a".repeat(32)));

        // Written past the repository, the schema still says no, and to a host
        // that is not a URL as well.
        for (label, host) in [
            ("Work", "https://api.github.com"),
            ("work", "api.github.com"),
        ] {
            let written = sqlx::query(
                "INSERT INTO integration_accounts (id, integration, label, host, created_at)
                 VALUES (?1, 'github', ?2, ?3, '2026-01-01T00:00:00Z')",
            )
            .bind(IntegrationAccountId::new().to_string())
            .bind(label)
            .bind(host)
            .execute(db.pool())
            .await;
            assert!(written.is_err(), "{label} at {host} was stored");
        }
        assert!(repo.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_hands_back_the_row_so_the_caller_can_remove_its_credential() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        let account = work();
        repo.insert(&account).await.unwrap();

        let deleted = repo.delete(account.id).await.unwrap();
        assert_eq!(deleted, account);
        assert!(repo.list().await.unwrap().is_empty());
        assert!(matches!(
            repo.delete(account.id).await.unwrap_err(),
            DbError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn touch_records_the_last_use() {
        let db = Database::in_memory().await.unwrap();
        let repo = db.integrations();
        let account = work();
        repo.insert(&account).await.unwrap();
        assert_eq!(repo.get(account.id).await.unwrap().last_used_at, None);

        repo.touch(account.id).await.unwrap();
        let used = repo.get(account.id).await.unwrap().last_used_at.unwrap();
        assert!(used >= account.created_at);
        assert!(matches!(
            repo.touch(IntegrationAccountId::new()).await.unwrap_err(),
            DbError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn the_token_has_no_column_to_live_in() {
        let db = Database::in_memory().await.unwrap();
        let columns: Vec<String> = sqlx::query("SELECT name FROM pragma_table_info(?1)")
            .bind(TABLE)
            .fetch_all(db.pool())
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();
        assert_eq!(
            columns,
            vec![
                "id",
                "integration",
                "label",
                "host",
                "private_network",
                "scopes",
                "created_at",
                "last_used_at",
            ]
        );
    }
}
