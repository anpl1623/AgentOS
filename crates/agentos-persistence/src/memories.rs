//! Agent memory.
//!
//! Keyword matching over structured rows, deliberately. A vector index is a
//! reasonable second implementation, not a reasonable first one: the interface
//! here ([`agentos_core::memory::MemoryQuery`] in, ranked memories out) is the
//! same shape a semantic store would satisfy, so swapping it later touches this
//! file and nothing else.

use agentos_core::ids::{AgentId, MemoryId};
use agentos_core::memory::{Memory, MemoryKind, MemoryQuery};
use agentos_core::trust::DataSource;
use sqlx::{Row, SqlitePool};

use crate::convert::{
    read_id, read_json, read_optional_id, read_time, read_unit_enum, write_json, write_time,
};
use crate::error::DbError;

const TABLE: &str = "memories";

/// Reads and writes agent memory.
#[derive(Debug, Clone)]
pub struct MemoryRepository {
    pool: SqlitePool,
}

impl MemoryRepository {
    pub(crate) const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a memory.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] if the agent does not exist.
    pub async fn insert(&self, memory: &Memory) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO memories (id, agent_id, kind, content, source, confidence,
                                   task_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(memory.id.to_string())
        .bind(memory.agent_id.to_string())
        .bind(memory.kind.as_str())
        .bind(&memory.content)
        .bind(write_json("source", &memory.source)?)
        .bind(f64::from(memory.confidence))
        .bind(memory.task_id.map(|id| id.to_string()))
        .bind(write_time(&memory.created_at))
        .bind(write_time(&memory.updated_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retrieve memories matching a query, most recently updated first.
    ///
    /// Free text is matched case-insensitively against the content. Ranking is
    /// recency, not relevance — an honest limitation, and better than a
    /// relevance score that is really just recency wearing a hat.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn query(&self, query: &MemoryQuery) -> Result<Vec<Memory>, DbError> {
        // `QueryBuilder` rather than string concatenation: every dynamic value
        // becomes a bind parameter, so there is no path from a filter value into
        // the SQL text.
        let mut builder =
            sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT * FROM memories WHERE 1 = 1");

        if let Some(agent_id) = query.agent_id {
            builder
                .push(" AND agent_id = ")
                .push_bind(agent_id.to_string());
        }
        if let Some(text) = &query.text {
            builder
                .push(" AND content LIKE ")
                .push_bind(format!("%{}%", escape_like(text)))
                .push(" ESCAPE '\\'");
        }
        if let Some(confidence) = query.min_confidence {
            builder
                .push(" AND confidence >= ")
                .push_bind(f64::from(confidence));
        }
        if !query.kinds.is_empty() {
            builder.push(" AND kind IN (");
            let mut separated = builder.separated(", ");
            for kind in &query.kinds {
                separated.push_bind(kind.as_str());
            }
            builder.push(")");
        }
        builder
            .push(" ORDER BY updated_at DESC LIMIT ")
            .push_bind(i64::try_from(query.limit).unwrap_or(i64::MAX));

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.iter().map(hydrate).collect()
    }

    /// Every memory for an agent, most recently updated first.
    ///
    /// # Errors
    ///
    /// [`DbError::Sql`] on failure.
    pub async fn list_for_agent(&self, agent_id: AgentId) -> Result<Vec<Memory>, DbError> {
        self.query(&MemoryQuery {
            agent_id: Some(agent_id),
            limit: i64::MAX as usize,
            ..MemoryQuery::default()
        })
        .await
    }

    /// Revise a memory's content and confidence.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn update(
        &self,
        id: MemoryId,
        content: &str,
        confidence: f32,
    ) -> Result<(), DbError> {
        let affected = sqlx::query(
            "UPDATE memories SET content = ?2, confidence = ?3, updated_at = ?4 WHERE id = ?1",
        )
        .bind(id.to_string())
        .bind(content)
        .bind(f64::from(confidence.clamp(0.0, 1.0)))
        .bind(write_time(&agentos_core::now()))
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "memory",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Forget a memory.
    ///
    /// # Errors
    ///
    /// [`DbError::NotFound`] if absent.
    pub async fn delete(&self, id: MemoryId) -> Result<(), DbError> {
        let affected = sqlx::query("DELETE FROM memories WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected == 0 {
            return Err(DbError::NotFound {
                entity: "memory",
                id: id.to_string(),
            });
        }
        Ok(())
    }
}

/// Escape `LIKE` wildcards so a search for `100%` does not match everything.
fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn hydrate(row: &sqlx::sqlite::SqliteRow) -> Result<Memory, DbError> {
    let confidence: f64 = row.try_get("confidence")?;
    Ok(Memory {
        id: read_id(TABLE, "id", row.try_get::<String, _>("id")?.as_str())?,
        agent_id: read_id(
            TABLE,
            "agent_id",
            row.try_get::<String, _>("agent_id")?.as_str(),
        )?,
        kind: read_unit_enum::<MemoryKind>(
            TABLE,
            "kind",
            row.try_get::<String, _>("kind")?.as_str(),
        )?,
        content: row.try_get("content")?,
        source: read_json::<DataSource>(
            TABLE,
            "source",
            row.try_get::<String, _>("source")?.as_str(),
        )?,
        confidence: confidence as f32,
        task_id: read_optional_id(TABLE, "task_id", row.try_get("task_id")?)?,
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
mod tests {
    use super::*;
    use crate::Database;
    use crate::agents::tests::sample_agent;

    async fn seeded() -> (Database, AgentId) {
        let db = Database::in_memory().await.unwrap();
        let agent = sample_agent("rememberer");
        db.agents().insert(&agent).await.unwrap();
        (db, agent.id)
    }

    #[tokio::test]
    async fn round_trips_including_provenance() {
        let (db, agent_id) = seeded().await;
        let memory = Memory::new(
            agent_id,
            MemoryKind::Observation,
            "The CRM lists 3 overdue accounts.",
            DataSource::Web {
                url: "http://localhost:8420/customers".into(),
            },
        )
        .with_confidence(0.6);
        db.memories().insert(&memory).await.unwrap();

        let loaded = db.memories().list_for_agent(agent_id).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, memory.content);
        assert!((loaded[0].confidence - 0.6).abs() < f32::EPSILON);
        assert!(loaded[0].is_from_untrusted_source());
        assert_eq!(
            loaded[0].source,
            DataSource::Web {
                url: "http://localhost:8420/customers".into()
            }
        );
    }

    #[tokio::test]
    async fn queries_filter_by_kind_text_and_confidence() {
        let (db, agent_id) = seeded().await;
        for (kind, content, confidence) in [
            (MemoryKind::Fact, "Acme pays on net-30 terms", 0.9),
            (
                MemoryKind::Preference,
                "Operator prefers concise reports",
                1.0,
            ),
            (MemoryKind::Observation, "Acme site was slow today", 0.2),
        ] {
            db.memories()
                .insert(
                    &Memory::new(agent_id, kind, content, DataSource::User)
                        .with_confidence(confidence),
                )
                .await
                .unwrap();
        }

        let by_kind = db
            .memories()
            .query(&MemoryQuery::for_agent(agent_id).of_kinds([MemoryKind::Fact]))
            .await
            .unwrap();
        assert_eq!(by_kind.len(), 1);
        assert_eq!(by_kind[0].kind, MemoryKind::Fact);

        let by_text = db
            .memories()
            .query(&MemoryQuery::for_agent(agent_id).matching("acme"))
            .await
            .unwrap();
        assert_eq!(by_text.len(), 2, "text matching must be case-insensitive");

        let confident = db
            .memories()
            .query(&MemoryQuery {
                min_confidence: Some(0.5),
                ..MemoryQuery::for_agent(agent_id)
            })
            .await
            .unwrap();
        assert_eq!(confident.len(), 2);
    }

    #[tokio::test]
    async fn like_wildcards_in_search_text_are_escaped() {
        // A search for `100%` must not degenerate into "match everything".
        let (db, agent_id) = seeded().await;
        db.memories()
            .insert(&Memory::new(
                agent_id,
                MemoryKind::Fact,
                "margin is 100% of target",
                DataSource::User,
            ))
            .await
            .unwrap();
        db.memories()
            .insert(&Memory::new(
                agent_id,
                MemoryKind::Fact,
                "unrelated note",
                DataSource::User,
            ))
            .await
            .unwrap();

        let results = db
            .memories()
            .query(&MemoryQuery::for_agent(agent_id).matching("100%"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn queries_are_scoped_to_one_agent() {
        let (db, agent_id) = seeded().await;
        let other = sample_agent("other");
        db.agents().insert(&other).await.unwrap();

        db.memories()
            .insert(&Memory::new(
                agent_id,
                MemoryKind::Fact,
                "mine",
                DataSource::User,
            ))
            .await
            .unwrap();
        db.memories()
            .insert(&Memory::new(
                other.id,
                MemoryKind::Fact,
                "theirs",
                DataSource::User,
            ))
            .await
            .unwrap();

        let mine = db.memories().list_for_agent(agent_id).await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].content, "mine");
    }

    #[tokio::test]
    async fn limits_are_respected() {
        let (db, agent_id) = seeded().await;
        for i in 0..10 {
            db.memories()
                .insert(&Memory::new(
                    agent_id,
                    MemoryKind::Fact,
                    format!("fact {i}"),
                    DataSource::User,
                ))
                .await
                .unwrap();
        }
        let limited = db
            .memories()
            .query(&MemoryQuery::for_agent(agent_id).limited_to(3))
            .await
            .unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[tokio::test]
    async fn memories_can_be_revised_and_forgotten() {
        let (db, agent_id) = seeded().await;
        let memory = Memory::new(agent_id, MemoryKind::Fact, "old", DataSource::User);
        db.memories().insert(&memory).await.unwrap();

        db.memories().update(memory.id, "new", 0.5).await.unwrap();
        let loaded = db.memories().list_for_agent(agent_id).await.unwrap();
        assert_eq!(loaded[0].content, "new");

        db.memories().delete(memory.id).await.unwrap();
        assert!(
            db.memories()
                .list_for_agent(agent_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            db.memories().delete(memory.id).await.unwrap_err(),
            DbError::NotFound { .. }
        ));
    }
}
