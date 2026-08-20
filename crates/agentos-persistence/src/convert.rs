//! Conversions between SQLite text columns and domain types.
//!
//! SQLite has no UUID, no timestamp and no JSON type worth relying on, so the
//! schema stores text and this module does the parsing in one place. Every
//! failure produces a [`DbError::CorruptRow`] naming the exact table and column,
//! because a schema/code mismatch is a bug worth locating instantly.

use std::str::FromStr;

use agentos_core::Timestamp;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::DbError;

/// Format a timestamp for storage. RFC3339 in UTC sorts lexicographically.
pub(crate) fn write_time(value: &Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Format an optional timestamp.
pub(crate) fn write_optional_time(value: Option<&Timestamp>) -> Option<String> {
    value.map(write_time)
}

/// Parse a timestamp column.
pub(crate) fn read_time(
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<Timestamp, DbError> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| DbError::corrupt(table, column, value, error))
}

/// Parse an optional timestamp column.
pub(crate) fn read_optional_time(
    table: &'static str,
    column: &'static str,
    value: Option<String>,
) -> Result<Option<Timestamp>, DbError> {
    value.map(|raw| read_time(table, column, &raw)).transpose()
}

/// Parse an identifier column.
pub(crate) fn read_id<T>(
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<T, DbError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(value).map_err(|error| DbError::corrupt(table, column, value, error))
}

/// Parse an optional identifier column.
pub(crate) fn read_optional_id<T>(
    table: &'static str,
    column: &'static str,
    value: Option<String>,
) -> Result<Option<T>, DbError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    value.map(|raw| read_id(table, column, &raw)).transpose()
}

/// Parse a JSON column.
pub(crate) fn read_json<T: DeserializeOwned>(
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<T, DbError> {
    serde_json::from_str(value).map_err(|error| DbError::corrupt(table, column, value, error))
}

/// Parse an optional JSON column.
pub(crate) fn read_optional_json<T: DeserializeOwned>(
    table: &'static str,
    column: &'static str,
    value: Option<String>,
) -> Result<Option<T>, DbError> {
    value.map(|raw| read_json(table, column, &raw)).transpose()
}

/// Serialise a value for a JSON column.
pub(crate) fn write_json<T: Serialize>(column: &'static str, value: &T) -> Result<String, DbError> {
    serde_json::to_string(value).map_err(|error| DbError::serialisation(column, error))
}

/// Parse an enum-like column via `FromStr`.
pub(crate) fn read_enum<T>(
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<T, DbError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(value).map_err(|error| DbError::corrupt(table, column, value, error))
}

/// Parse a unit-variant enum stored as its `serde` snake_case name.
///
/// Used for the domain enums that render via `as_str` but have no `FromStr`;
/// going through `serde` keeps the stored spelling and the wire spelling as one
/// definition instead of two that can drift.
pub(crate) fn read_unit_enum<T: DeserializeOwned>(
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<T, DbError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|error| DbError::corrupt(table, column, value, error))
}
