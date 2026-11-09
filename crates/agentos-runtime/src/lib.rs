//! The AgentOS runtime.
//!
//! This is the composition root and the only place the pieces meet: the
//! database, the audit log, the tool registry, the policy engine, the provider
//! and the agent loop.
//!
//! It is also the API. The CLI in `agentos-cli` and, later, the desktop
//! application are both clients of this type — there is no second
//! implementation of any of this behind either of them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent_loop;
pub mod budget;
pub mod config;
pub mod credentials;
pub mod delegation;
pub mod error;
pub mod gate;
pub mod grants;
pub mod integrations;
mod liveness;
pub mod operator;
pub mod prompt;
mod registry;
mod runtime;
pub mod scheduler;
pub mod state;

pub use agent_loop::{AgentLoop, RunOutcome};
pub use agentos_permissions::Reach;
pub use agentos_tools::ToolRegistry as Registry;
pub use budget::RunBudget;
pub use config::{
    DelegationLimits, FixedProviderFactory, ProviderFactory, RuntimeConfig,
    SecretBackedProviderFactory, build_provider,
};
pub use credentials::SecretStoreResolver;
pub use delegation::{DelegateTool, DelegationBroker, ParentRunHandle};
pub use error::RuntimeError;
pub use gate::RunApprovalGate;
pub use grants::{CapabilityGrant, ToolGrant};
pub use integrations::SqliteAccountDirectory;
pub use registry::{
    build_registry, build_registry_for, build_registry_sharing, build_registry_sharing_with,
    compose_registry, compose_registry_with,
};
pub use runtime::{
    AuditCheckpoint, DELEGATED_TASK, DelegationNode, PreparedRun, RunTrace, Runtime,
};
pub use scheduler::{
    MIN_TICK_SECONDS, Scheduler, SchedulerOptions, SchedulerPreference, SchedulerTransition,
    TickReport,
};
pub use state::RunStateMachine;
