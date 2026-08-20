//! Agents and their policies.

use agentos_core::agent::{Agent, AgentStatus, ModelConfig};
use agentos_core::ids::AgentId;
use sqlx::{Row, SqlitePool};

use crate::convert::{read_id, read_json, read_time, read_unit_enum, write_json, write_time};
use crate::error::DbError;

const TABLE: &str = "agents";

/// Reads and writes agents.
#[derive(Debug, Clone)]
pub struct AgentRepository {
    pool: SqlitePool,
}

/// A stored policy document with its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPolicy {
    /// The YAML source, preserved verbatim including comments.
    pub document: String,
    /// Incremented on each save, so a stale UI can detect it lost a race.
    pub version: i64,
}

impl AgentRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create an agent.
    ///
    /// # Errors
    ///
    /// [`DbError::Conflict`] if the name is taken; [`DbError::Sql`] otherwise.
    pub async fn insert(&self, agent: &Agent) -> Result<(), DbError> {
        let result = sqlx::query(
            "INSERT INTO agents (id, name, instructions, provider, model, temperature,
                                 max_output_tokens, base_url, enabled_tools, status,
                                 max_steps, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )
        .bind(agent.id.to_string())
        .bind(&agent.name)
        .bind(&agent.instructions)
        .bind(&agent.model.provider)
        .bind(&agent.model.model)
        .bind(agent.model.temperature)
        .bind(agent.model.max_output_tokens)
        .bind(agent.model.base_url.as_deref())
        .bind(write_json("enabled_tools", &agent.enabled_tools)?)
        .bind(agent.status.as_str())
        .bind(agent.max_steps)
        .bind(write_json("metadata", &agent.metadata)?)
        .bind(write_time(&agent.created_at))
        .bind(write_time(&agent.updated_at))
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(DbError::Conflict {
                    entity: "agent",
                    value: agent.name.clone(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Overwrite an agent's mutable fields.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if the agent does not exist.
    pub async fn update(&self, agent: &Agent) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE agents SET name = ?2, instructions = ?3, provider = ?4, model = ?5,
                               temperature = ?6, max_output_tokens = ?7, base_url = ?8,
                               enabled_tools = ?9, status = ?10, max_steps = ?11,
                               metadata = ?12, updated_at = ?13
             WHERE id = ?1",
        )
        .bind(agent.id.to_string())
        .bind(&agent.name)
        .bind(&agent.instructions)
        .bind(&agent.model.provider)
        .bind(&agent.model.model)
        .bind(agent.model.temperature)
        .bind(agent.model.max_output_tokens)
        .bind(agent.model.base_url.as_deref())
        .bind(write_json("enabled_tools", &agent.enabled_tools)?)
        .bind(agent.status.as_str())
        .bind(agent.max_steps)
        .bind(write_json("metadata", &agent.metadata)?)
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "agent",
                id: agent.id.to_string(),
            });
        }
        Ok(())
    }

    /// Fetch an agent by id.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn get(&self, id: AgentId) -> Result<Agent, DbError> {
        self.find(id).await?.ok_or(DbError::NotFound {
            entity: "agent",
            id: id.to_string(),
        })
    }

    /// Fetch an agent by id, or `None`.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find(&self, id: AgentId) -> Result<Option<Agent>, DbError> {
        let row = sqlx::query("SELECT * FROM agents WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// Fetch an agent by name.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn find_by_name(&self, name: &str) -> Result<Option<Agent>, DbError> {
        let row = sqlx::query("SELECT * FROM agents WHERE name = ?1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| hydrate(&row)).transpose()
    }

    /// All agents, newest first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list(&self) -> Result<Vec<Agent>, DbError> {
        let rows = sqlx::query("SELECT * FROM agents ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(hydrate).collect()
    }

    /// Delete an agent and everything owned by it.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn delete(&self, id: AgentId) -> Result<(), DbError> {
        let affected = sqlx::query("DELETE FROM agents WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "agent",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Store an agent's policy document, bumping its version.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn set_policy(&self, agent_id: AgentId, document: &str) -> Result<i64, DbError> {
        let row = sqlx::query(
            "INSERT INTO policies (agent_id, document, version, updated_at)
             VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(agent_id) DO UPDATE
               SET document = excluded.document,
                   version = policies.version + 1,
                   updated_at = excluded.updated_at
             RETURNING version",
        )
        .bind(agent_id.to_string())
        .bind(document)
        .bind(write_time(&agentos_core::now()))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("version")?)
    }

    /// Read an agent's policy document.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn policy(&self, agent_id: AgentId) -> Result<Option<StoredPolicy>, DbError> {
        let row = sqlx::query("SELECT document, version FROM policies WHERE agent_id = ?1")
            .bind(agent_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            Ok(StoredPolicy {
                document: row.try_get("document")?,
                version: row.try_get("version")?,
            })
        })
        .transpose()
    }
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Agent, DbError> {
    let enabled_tools: Vec<String> = read_json(
        TABLE,
        "enabled_tools",
        row.try_get::<String, _>("enabled_tools")?.as_str(),
    )?;

    Ok(Agent {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        name: row.try_get("name")?,
        instructions: row.try_get("instructions")?,
        model: ModelConfig {
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            temperature: row.try_get("temperature")?,
            max_output_tokens: row.try_get("max_output_tokens")?,
            base_url: row.try_get("base_url")?,
        },
        enabled_tools,
        status: read_unit_enum::<AgentStatus>(
            TABLE,
            "status",
            row.try_get::<String, _>("status")?.as_str(),
        )?,
        max_steps: row.try_get("max_steps")?,
        metadata: read_json(
            TABLE,
            "metadata",
            row.try_get::<String, _>("metadata")?.as_str(),
        )?,
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Database;

    pub(crate) fn sample_agent(name: &str) -> Agent {
        Agent::new(
            name,
            "You are a careful operations agent.",
            ModelConfig::new("mock", "scripted"),
        )
        .with_tools(["filesystem.read", "filesystem.write"])
    }

    #[tokio::test]
    async fn round_trips_every_field() {
        let db = Database::in_memory().await.unwrap();
        let mut agent = sample_agent("sales");
        agent.model.temperature = Some(0.3);
        agent.model.max_output_tokens = Some(2048);
        agent.model.base_url = Some("http://localhost:11434/v1".into());
        agent.max_steps = 12;
        agent.metadata = serde_json::json!({"team": "revenue"});

        db.agents().insert(&agent).await.unwrap();
        let loaded = db.agents().get(agent.id).await.unwrap();

        assert_eq!(loaded.name, agent.name);
        assert_eq!(loaded.instructions, agent.instructions);
        assert_eq!(loaded.model, agent.model);
        assert_eq!(loaded.enabled_tools, agent.enabled_tools);
        assert_eq!(loaded.max_steps, 12);
        assert_eq!(loaded.metadata, agent.metadata);
        assert_eq!(loaded.status, AgentStatus::Enabled);
    }

    #[tokio::test]
    async fn duplicate_names_are_rejected() {
        let db = Database::in_memory().await.unwrap();
        db.agents().insert(&sample_agent("dup")).await.unwrap();
        let err = db.agents().insert(&sample_agent("dup")).await.unwrap_err();
        assert!(matches!(
            err,
            DbError::Conflict {
                entity: "agent",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn find_by_name_and_missing_lookups() {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("ops");
        db.agents().insert(&agent).await.unwrap();

        assert_eq!(
            db.agents().find_by_name("ops").await.unwrap().map(|a| a.id),
            Some(agent.id)
        );
        assert!(db.agents().find_by_name("nobody").await.unwrap().is_none());
        assert!(matches!(
            db.agents().get(AgentId::new()).await.unwrap_err(),
            DbError::NotFound {
                entity: "agent",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn updates_are_applied() {
        let db = Database::in_memory().await.unwrap();
        let mut agent = sample_agent("editable");
        db.agents().insert(&agent).await.unwrap();

        agent.instructions = "Revised instructions.".into();
        agent.status = AgentStatus::Disabled;
        agent.enabled_tools = vec!["terminal.exec".into()];
        db.agents().update(&agent).await.unwrap();

        let loaded = db.agents().get(agent.id).await.unwrap();
        assert_eq!(loaded.instructions, "Revised instructions.");
        assert_eq!(loaded.status, AgentStatus::Disabled);
        assert!(!loaded.is_enabled());
        assert_eq!(loaded.enabled_tools, vec!["terminal.exec".to_owned()]);
    }

    #[tokio::test]
    async fn updating_a_missing_agent_is_an_error() {
        let db = Database::in_memory().await.unwrap();
        let err = db
            .agents()
            .update(&sample_agent("ghost"))
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::NotFound { .. }));
    }

    #[tokio::test]
    async fn policies_are_versioned_and_preserved_verbatim() {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("policied");
        db.agents().insert(&agent).await.unwrap();

        let document = "# a comment the operator wrote\ndefault: deny\npermissions: {}\n";
        assert_eq!(db.agents().set_policy(agent.id, document).await.unwrap(), 1);

        let stored = db.agents().policy(agent.id).await.unwrap().unwrap();
        assert_eq!(
            stored.document, document,
            "comments must survive a round trip"
        );
        assert_eq!(stored.version, 1);

        assert_eq!(
            db.agents()
                .set_policy(agent.id, "default: ask\n")
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            db.agents().policy(agent.id).await.unwrap().unwrap().version,
            2
        );
    }

    #[tokio::test]
    async fn deleting_an_agent_removes_its_policy() {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("doomed");
        db.agents().insert(&agent).await.unwrap();
        db.agents()
            .set_policy(agent.id, "default: deny\n")
            .await
            .unwrap();

        db.agents().delete(agent.id).await.unwrap();
        assert!(db.agents().policy(agent.id).await.unwrap().is_none());
        assert!(matches!(
            db.agents().delete(agent.id).await.unwrap_err(),
            DbError::NotFound { .. }
        ));
    }
}
