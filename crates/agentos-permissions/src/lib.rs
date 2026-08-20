//! The AgentOS permission system.
//!
//! Permissions are enforced by this crate, in the runtime, before any tool runs.
//! They are not described to the model and hoped for. The model's only influence
//! on a decision is the already-validated argument it supplied, which the
//! runtime turns into a resource reference; everything else — the policy, the
//! tool's declared requirements, the taint state — comes from outside the model.
//!
//! Three pieces:
//!
//! * [`path`] resolves filesystem paths safely, so that scoping survives `..`
//!   and symlinks.
//! * [`policy`] holds the rules and the specificity ordering that resolves
//!   conflicts between them.
//! * [`engine`] evaluates a request against a policy and produces a decision.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod engine;
pub mod error;
pub mod path;
pub mod pattern;
pub mod policy;
pub mod yaml;

pub use engine::{DenyAllEngine, PermissionEngine, PolicyEngine};
pub use error::{PathError, PolicyError};
pub use pattern::{GlobKind, NamePattern, ResourcePattern};
pub use policy::{IMMUTABLE_DENY, Policy, PolicyRule, TaintPolicy, is_immutably_denied};
pub use yaml::{PolicyDocument, load_policy_file, starter_policy_yaml};
