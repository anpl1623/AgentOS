//! Command implementations.

pub mod agent;
pub mod audit;
pub mod demo;
pub mod doctor;
pub mod policy;
pub mod provider;
pub mod schedule;
pub mod task;
pub mod tools;

use std::sync::Arc;

use agentos_runtime::{Runtime, RuntimeConfig};
use agentos_secrets::ChainSecretStore;
use anyhow::Result;

/// Open the runtime for a command.
pub async fn open(config: &RuntimeConfig) -> Result<Runtime> {
    Ok(Runtime::open_with_secrets(config.clone(), Arc::new(ChainSecretStore::standard())).await?)
}
