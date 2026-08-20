//! Persistence errors.

use thiserror::Error;

/// Something went wrong talking to the database.
#[derive(Debug, Error)]
pub enum DbError {
    /// The underlying SQL call failed.
    #[error("database error: {0}")]
    Sql(#[from] sqlx::Error),

    /// Schema migration failed.
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    /// A row held a value the domain types could not accept.
    ///
    /// This means the database and the code have drifted, which is worth a
    /// distinct error rather than being folded into a generic failure.
    #[error("corrupt row in `{table}`: column `{column}` held `{value}`: {reason}")]
    CorruptRow {
        /// Table the row came from.
        table: &'static str,
        /// Offending column.
        column: &'static str,
        /// What was stored.
        value: String,
        /// Why it could not be used.
        reason: String,
    },

    /// A lookup found nothing.
    #[error("no {entity} with id `{id}`")]
    NotFound {
        /// Entity kind.
        entity: &'static str,
        /// The identifier looked up.
        id: String,
    },

    /// A uniqueness constraint was violated.
    #[error("{entity} `{value}` already exists")]
    Conflict {
        /// Entity kind.
        entity: &'static str,
        /// The conflicting value.
        value: String,
    },

    /// A value could not be serialised for storage.
    #[error("cannot serialise value for `{column}`: {source}")]
    Serialisation {
        /// Target column.
        column: &'static str,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
}

impl DbError {
    /// Build a corrupt-row error.
    pub(crate) fn corrupt(
        table: &'static str,
        column: &'static str,
        value: impl Into<String>,
        reason: impl std::fmt::Display,
    ) -> Self {
        Self::CorruptRow {
            table,
            column,
            value: value.into(),
            reason: reason.to_string(),
        }
    }

    /// Build a serialisation error.
    pub(crate) const fn serialisation(column: &'static str, source: serde_json::Error) -> Self {
        Self::Serialisation { column, source }
    }
}
