//! Filesystem tools.
//!
//! Every path an agent supplies is resolved through
//! [`agentos_permissions::path::resolve_secure`] before it becomes a
//! [`ResourceRef`], so the policy engine is always deciding about the location
//! the operation will actually reach — not the string the model typed. `..` and
//! symlinks are therefore not special cases here; they are resolved away before
//! any decision is made.
//!
//! Risk is per-call, not per-tool: creating a file is `medium`, overwriting an
//! existing one is `high`, and a recursive delete is `critical`.
//!
//! `filesystem.search` is the one tool here that cannot name everything it
//! touches in its plan. See [`SearchFiles`] for how it stays inside what the
//! policy allows while it walks.

mod read;
mod search;
#[cfg(test)]
mod test_support;
mod transfer;
mod write;

use std::path::PathBuf;

use agentos_core::permission::{Capability, ResourceRef, permission_domains};
use agentos_permissions::path::{expand_home, resolve_secure};

use crate::error::ToolError;
use crate::tool::{Tool, ToolContext};

pub use read::{ListArgs, ListDirectory, ReadArgs, ReadFile};
pub use search::{
    MAX_SEARCH_DEPTH, MAX_SEARCH_RESULTS, MAX_SEARCHED_FILE_BYTES, SearchArgs, SearchFiles,
};
pub use transfer::{CopyFile, MoveFile, TransferArgs};
pub use write::{DeleteArgs, DeletePath, WriteArgs, WriteFile};

/// Resolve an agent-supplied path to the location it will really reach.
///
/// Relative paths are anchored to the agent's workspace. Being in the workspace
/// grants nothing — the policy still decides — it only fixes what a relative
/// path means.
fn resolve(context: &ToolContext, raw: &str) -> Result<PathBuf, ToolError> {
    let expanded = expand_home(raw).ok_or_else(|| {
        ToolError::Failed(format!("cannot expand `~` in `{raw}`: no home directory"))
    })?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        context.workspace.join(expanded)
    };
    resolve_secure(&absolute).map_err(ToolError::Path)
}

fn path_resource(path: &std::path::Path) -> ResourceRef {
    ResourceRef::Path {
        path: path.display().to_string(),
    }
}

fn capability(action: &str, path: &std::path::Path) -> Capability {
    Capability::new(permission_domains::FILESYSTEM, action).with_resource(path_resource(path))
}

/// Every filesystem tool, ready to register.
#[must_use]
pub fn all() -> Vec<std::sync::Arc<dyn Tool>> {
    vec![
        std::sync::Arc::new(ReadFile::new()),
        std::sync::Arc::new(WriteFile::new()),
        std::sync::Arc::new(ListDirectory::new()),
        std::sync::Arc::new(SearchFiles::new()),
        std::sync::Arc::new(DeletePath::new()),
        std::sync::Arc::new(CopyFile::new()),
        std::sync::Arc::new(MoveFile::new()),
    ]
}
