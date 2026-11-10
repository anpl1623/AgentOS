//! `filesystem.write` and `filesystem.delete`: the tools that change one path
//! in place.
//!
//! Both set the call's risk from what it would destroy. Overwriting an
//! existing file is riskier than creating one, and a recursive delete is
//! riskier than removing a single entry, so the policy can permit the lesser
//! without the greater.

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

/// Arguments for `filesystem.write`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteArgs {
    /// Path to write.
    pub path: String,
    /// Contents to write.
    pub content: String,
    /// Append rather than replace. Defaults to false.
    #[serde(default)]
    pub append: bool,
}

/// Writes a file.
#[derive(Debug)]
pub struct WriteFile(ToolMetadata);

impl Default for WriteFile {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFile {
    /// Build the tool.
    #[must_use]
    pub fn new() -> Self {
        Self(metadata_for::<WriteArgs>(
            "filesystem.write",
            "Write text to a file, creating parent directories as needed. Set `append` to add \
             to an existing file instead of replacing it.",
            RiskLevel::Medium,
            vec![Capability::new(permission_domains::FILESYSTEM, "write")],
            false,
        ))
    }
}

#[async_trait]
impl Tool for WriteFile {
    fn metadata(&self) -> &ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: WriteArgs = parse_arguments(&self.0.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: WriteArgs = parse_arguments(&self.0.name, arguments)?;
        let path = resolve(context, &args.path)?;

        // Replacing existing content is destructive; creating a new file is not.
        let overwrites = !args.append && tokio::fs::try_exists(&path).await.unwrap_or(false);
        let risk = if overwrites {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        };
        let verb = if args.append {
            "Append to"
        } else if overwrites {
            "Overwrite"
        } else {
            "Create"
        };

        Ok(ToolPlan::new(
            risk,
            format!("{verb} {} ({} bytes)", path.display(), args.content.len()),
        )
        .requiring(capability("write", &path)))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: WriteArgs = parse_arguments(&self.0.name, &arguments)?;
        let path = resolve(context, &args.path)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                ToolError::io(format!("creating {}", parent.display()), source)
            })?;
        }

        if args.append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|source| ToolError::io(format!("opening {}", path.display()), source))?;
            file.write_all(args.content.as_bytes())
                .await
                .map_err(|source| ToolError::io(format!("writing {}", path.display()), source))?;
            file.flush()
                .await
                .map_err(|source| ToolError::io(format!("flushing {}", path.display()), source))?;
        } else {
            tokio::fs::write(&path, &args.content)
                .await
                .map_err(|source| ToolError::io(format!("writing {}", path.display()), source))?;
        }

        Ok(ToolOutput::text(
            DataSource::Runtime,
            format!("Wrote {} bytes to {}", args.content.len(), path.display()),
        )
        .with_structured(serde_json::json!({
            "path": path.display().to_string(),
            "bytes": args.content.len(),
            "appended": args.append,
        })))
    }
}

/// Arguments for `filesystem.delete`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    /// Path to remove.
    pub path: String,
    /// Remove a directory and everything under it. Defaults to false.
    #[serde(default)]
    pub recursive: bool,
}

/// Deletes a file or directory.
#[derive(Debug)]
pub struct DeletePath(ToolMetadata);

impl Default for DeletePath {
    fn default() -> Self {
        Self::new()
    }
}

impl DeletePath {
    /// Build the tool.
    #[must_use]
    pub fn new() -> Self {
        Self(metadata_for::<DeleteArgs>(
            "filesystem.delete",
            "Delete a file, or a directory and its contents when `recursive` is set. This \
             cannot be undone.",
            RiskLevel::High,
            vec![Capability::new(permission_domains::FILESYSTEM, "delete")],
            false,
        ))
    }
}

#[async_trait]
impl Tool for DeletePath {
    fn metadata(&self) -> &ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: DeleteArgs = parse_arguments(&self.0.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: DeleteArgs = parse_arguments(&self.0.name, arguments)?;
        let path = resolve(context, &args.path)?;

        // Removing a tree is categorically worse than removing a file, and the
        // policy should be able to permit one without the other.
        let risk = if args.recursive {
            RiskLevel::Critical
        } else {
            RiskLevel::High
        };
        let summary = if args.recursive {
            format!(
                "Recursively delete {} and everything inside it",
                path.display()
            )
        } else {
            format!("Delete {}", path.display())
        };

        Ok(ToolPlan::new(risk, summary).requiring(capability("delete", &path)))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: DeleteArgs = parse_arguments(&self.0.name, &arguments)?;
        let path = resolve(context, &args.path)?;

        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|source| ToolError::io(format!("inspecting {}", path.display()), source))?;

        if metadata.is_dir() {
            if !args.recursive {
                return Err(ToolError::Failed(format!(
                    "{} is a directory; set `recursive` to delete it",
                    path.display()
                )));
            }
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|source| ToolError::io(format!("deleting {}", path.display()), source))?;
        } else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|source| ToolError::io(format!("deleting {}", path.display()), source))?;
        }

        Ok(ToolOutput::text(
            DataSource::Runtime,
            format!("Deleted {}", path.display()),
        ))
    }
}
