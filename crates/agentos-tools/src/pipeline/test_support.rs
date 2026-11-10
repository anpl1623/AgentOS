//! Fixtures shared by the pipeline's tests: an audit sink on a full disk, a
//! credential store that hands anything to anyone, and a tool that spends a
//! credential and echoes it back.

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::permission::{Capability, Effect, ResourceRef};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolCall;
use agentos_core::trust::DataSource;
use tokio_util::sync::CancellationToken;

use super::{ExecutionReport, ToolPipeline};
use crate::error::ToolError;
use crate::taint::TaintTracker;
use crate::tool::{CredentialResolver, Secret, ToolContext, ToolOutput, ToolPlan, ToolRegistry};

/// A sink on a disk that has stopped taking writes.
#[derive(Debug)]
pub(super) struct FullDisk;

#[async_trait::async_trait]
impl agentos_audit::AuditSink for FullDisk {
    async fn append(
        &self,
        _record: &agentos_audit::AuditRecord,
    ) -> Result<(), agentos_audit::AuditError> {
        Err(agentos_audit::AuditError::Sink("disk full".into()))
    }

    async fn tip(&self) -> Result<(u64, String), agentos_audit::AuditError> {
        Ok((0, agentos_audit::GENESIS_HASH.to_owned()))
    }
}

/// A live token, as distinctive as a real one.
pub(super) const TOKEN: &str = "ghp_7Xq2LmN9pR4sT6vW8yZ0aB1cD3eF5gH7";

/// A store that would hand any credential to anyone.
///
/// The binding under test is the pipeline's, so the store is made as
/// careless as possible.
#[derive(Debug)]
pub(super) struct Careless;

#[async_trait::async_trait]
impl CredentialResolver for Careless {
    async fn resolve(&self, _origin: &str, _name: &str) -> Option<Secret> {
        Some(Secret::new(TOKEN))
    }
}

/// Plans to spend `https://a.example/deploy`, and then does what its
/// arguments say: spend it or not, fail or not, or hold the request open
/// once it has the secret. Whatever it returns quotes the token, as a
/// service that echoes credentials would.
#[derive(Debug)]
pub(super) struct Spender(agentos_core::tool::ToolMetadata, Arc<tokio::sync::Notify>);

impl Spender {
    pub(super) fn new() -> Self {
        Self(
            crate::tool::metadata_for::<serde_json::Value>(
                "network.spend",
                "spends a credential",
                RiskLevel::Low,
                vec![Capability::new("network", "credential")],
                true,
            ),
            Arc::new(tokio::sync::Notify::new()),
        )
    }
}

#[async_trait::async_trait]
impl crate::Tool for Spender {
    fn metadata(&self) -> &agentos_core::tool::ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        _arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        if context.credentials.is_some() {
            return Err(ToolError::Failed("planned with credentials".into()));
        }
        Ok(ToolPlan::new(RiskLevel::Low, "spend").requiring(
            Capability::new("network", "credential").with_resource(ResourceRef::Named {
                name: "https://a.example/deploy".into(),
            }),
        ))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        if arguments["spend"] == true {
            let store = context.credentials.as_ref().expect("a store");
            // The plan named this credential at a.example and no other.
            if store.resolve("https://b.example", "deploy").await.is_some()
                || store.resolve("https://a.example", "admin").await.is_some()
            {
                return Err(ToolError::Failed("released an unplanned credential".into()));
            }
            let released = store.resolve("https://a.example", "deploy").await;
            if arguments["stall"] == true {
                // Holding the secret, the request in flight: as far as a
                // call gets before the process exits under it.
                self.1.notify_one();
                std::future::pending::<()>().await;
            }
            if released.is_none() {
                return Err(ToolError::Failed(
                    "the planned credential was refused".into(),
                ));
            }
        }
        if arguments["fail"] == true {
            return Err(ToolError::Failed(format!(
                "401: token {TOKEN} was rejected"
            )));
        }
        Ok(ToolOutput::text(
            DataSource::Web {
                url: format!("https://a.example/?echo={TOKEN}"),
            },
            format!("you sent {TOKEN}"),
        )
        .with_structured(serde_json::json!({
            "received": [TOKEN],
            TOKEN: true,
            "authorised": context
                .authorised
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        })))
    }
}

pub(super) async fn spending_pipeline()
-> (ToolPipeline, Arc<agentos_audit::InMemorySink>, ToolContext) {
    let (pipeline, sink, context, _) = spending_pipeline_with(Spender::new()).await;
    (pipeline, sink, context)
}

pub(super) async fn spending_pipeline_with(
    spender: Spender,
) -> (
    ToolPipeline,
    Arc<agentos_audit::InMemorySink>,
    ToolContext,
    Arc<tokio::sync::Notify>,
) {
    let (audit, sink) = AuditLog::in_memory().await.unwrap();
    let mut registry = ToolRegistry::new();
    let stalled = Arc::clone(&spender.1);
    registry.register(Arc::new(spender));
    let policy = agentos_permissions::Policy::deny_all("t").with_rule(
        agentos_permissions::policy::PolicyRule::new(
            "spend",
            "network",
            "credential",
            Effect::Allow,
        ),
    );
    let pipeline = ToolPipeline::new(
        Arc::new(registry),
        Arc::new(agentos_permissions::PolicyEngine::new(policy)),
        Arc::new(crate::DenyAllGate),
        Arc::new(audit),
    );
    let context = ToolContext::new(
        agentos_core::ids::AgentId::new(),
        agentos_core::ids::TaskId::new(),
        agentos_core::ids::TaskRunId::new(),
        std::env::temp_dir(),
    )
    .with_credentials(Arc::new(Careless));
    (pipeline, sink, context, stalled)
}

pub(super) async fn spend(
    pipeline: &ToolPipeline,
    context: &ToolContext,
    taint: &TaintTracker,
    arguments: serde_json::Value,
) -> ExecutionReport {
    pipeline
        .execute(
            &ToolCall::new("1", "network.spend", arguments),
            context,
            taint,
            "agent",
            &["network.spend".to_owned()],
            &CancellationToken::new(),
        )
        .await
}
