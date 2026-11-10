//! Fixtures shared by the filesystem tools' tests: a policy probe that denies
//! by path prefix, and a workspace with a secret directory inside it and
//! another directory outside it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::{Capability, ResourceRef};
use tempfile::TempDir;

use crate::tool::{PolicyProbe, ToolContext};

/// A policy that allows everything except what lies under its prefixes.
#[derive(Debug, Default)]
pub(super) struct Probe {
    /// `(action, prefix)`, where an action of `*` denies every action.
    denied: Vec<(&'static str, PathBuf)>,
}

impl Probe {
    pub(super) fn denying(mut self, action: &'static str, prefix: PathBuf) -> Self {
        self.denied.push((action, prefix));
        self
    }
}

impl PolicyProbe for Probe {
    fn permits(&self, capability: &Capability) -> bool {
        let Some(ResourceRef::Path { path }) = &capability.resource else {
            return false;
        };
        !self.denied.iter().any(|(action, prefix)| {
            (*action == "*" || *action == capability.action) && Path::new(path).starts_with(prefix)
        })
    }
}

/// A workspace with a secret directory inside it and another outside it.
pub(super) struct Tree {
    pub(super) root: PathBuf,
    pub(super) outside: PathBuf,
    _root_guard: TempDir,
    _outside_guard: TempDir,
}

impl Tree {
    pub(super) fn new() -> Self {
        let root_guard = TempDir::new().unwrap();
        let root = std::fs::canonicalize(root_guard.path()).unwrap();
        let outside_guard = TempDir::new().unwrap();
        let outside = std::fs::canonicalize(outside_guard.path()).unwrap();

        std::fs::write(root.join("notes.txt"), "alpha\n  a needle here  \n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() { needle(); }\n").unwrap();
        std::fs::create_dir_all(root.join("secrets/inner")).unwrap();
        std::fs::write(root.join("secrets/key.txt"), "needle secret-token\n").unwrap();
        std::fs::write(root.join("secrets/inner/deep.txt"), "needle deep-token\n").unwrap();
        std::fs::write(outside.join("secret.txt"), "needle outside-token\n").unwrap();

        Self {
            root,
            outside,
            _root_guard: root_guard,
            _outside_guard: outside_guard,
        }
    }

    pub(super) fn context(&self, probe: Probe) -> ToolContext {
        self.bare_context().with_policy(Arc::new(probe))
    }

    pub(super) fn bare_context(&self) -> ToolContext {
        ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            self.root.clone(),
        )
    }
}
