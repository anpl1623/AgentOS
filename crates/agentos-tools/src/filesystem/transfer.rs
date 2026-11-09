//! `filesystem.copy` and `filesystem.move`: the tools with two ends.
//!
//! Each authorises its source and its destination separately, because being
//! allowed to read a file says nothing about where it may be written. A move
//! needs `delete` at its source rather than `read`, since the source is gone
//! afterwards.

use agentos_core::permission::{Capability, permission_domains};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use agentos_core::trust::DataSource;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{capability, resolve};
use crate::error::ToolError;
use crate::tool::{Tool, ToolContext, ToolOutput, ToolPlan, metadata_for, parse_arguments};

/// Arguments for `filesystem.copy` and `filesystem.move`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferArgs {
    /// Source path.
    pub from: String,
    /// Destination path.
    pub to: String,
}

/// Copies a file.
#[derive(Debug)]
pub struct CopyFile(ToolMetadata);

impl Default for CopyFile {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyFile {
    /// Build the tool.
    #[must_use]
    pub fn new() -> Self {
        Self(metadata_for::<TransferArgs>(
            "filesystem.copy",
            "Copy a file to a new location.",
            RiskLevel::Medium,
            vec![
                Capability::new(permission_domains::FILESYSTEM, "read"),
                Capability::new(permission_domains::FILESYSTEM, "write"),
            ],
            false,
        ))
    }
}

#[async_trait]
impl Tool for CopyFile {
    fn metadata(&self) -> &ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: TransferArgs = parse_arguments(&self.0.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: TransferArgs = parse_arguments(&self.0.name, arguments)?;
        let from = resolve(context, &args.from)?;
        let to = resolve(context, &args.to)?;

        let risk = if tokio::fs::try_exists(&to).await.unwrap_or(false) {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        };

        // Both ends are authorised. Reading a file you may read and writing it
        // somewhere you may not is still an exfiltration.
        Ok(
            ToolPlan::new(risk, format!("Copy {} to {}", from.display(), to.display()))
                .requiring(capability("read", &from))
                .requiring(capability("write", &to)),
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: TransferArgs = parse_arguments(&self.0.name, &arguments)?;
        let from = resolve(context, &args.from)?;
        let to = resolve(context, &args.to)?;

        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                ToolError::io(format!("creating {}", parent.display()), source)
            })?;
        }
        let bytes = tokio::fs::copy(&from, &to).await.map_err(|source| {
            ToolError::io(
                format!("copying {} to {}", from.display(), to.display()),
                source,
            )
        })?;

        Ok(ToolOutput::text(
            DataSource::Runtime,
            format!("Copied {bytes} bytes to {}", to.display()),
        ))
    }
}

/// Moves a file.
#[derive(Debug)]
pub struct MoveFile(ToolMetadata);

impl Default for MoveFile {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveFile {
    /// Build the tool.
    #[must_use]
    pub fn new() -> Self {
        Self(metadata_for::<TransferArgs>(
            "filesystem.move",
            "Move or rename a file. The source no longer exists afterwards.",
            RiskLevel::High,
            vec![
                Capability::new(permission_domains::FILESYSTEM, "delete"),
                Capability::new(permission_domains::FILESYSTEM, "write"),
            ],
            false,
        ))
    }
}

#[async_trait]
impl Tool for MoveFile {
    fn metadata(&self) -> &ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: TransferArgs = parse_arguments(&self.0.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: TransferArgs = parse_arguments(&self.0.name, arguments)?;
        let from = resolve(context, &args.from)?;
        let to = resolve(context, &args.to)?;

        // A move removes the source, so it needs delete there, not merely read.
        Ok(ToolPlan::new(
            RiskLevel::High,
            format!("Move {} to {}", from.display(), to.display()),
        )
        .requiring(capability("delete", &from))
        .requiring(capability("write", &to)))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: TransferArgs = parse_arguments(&self.0.name, &arguments)?;
        let from = resolve(context, &args.from)?;
        let to = resolve(context, &args.to)?;

        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                ToolError::io(format!("creating {}", parent.display()), source)
            })?;
        }
        tokio::fs::rename(&from, &to).await.map_err(|source| {
            ToolError::io(
                format!("moving {} to {}", from.display(), to.display()),
                source,
            )
        })?;

        Ok(ToolOutput::text(
            DataSource::Runtime,
            format!("Moved {} to {}", from.display(), to.display()),
        ))
    }
}
