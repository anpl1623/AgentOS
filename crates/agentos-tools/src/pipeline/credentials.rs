//! Credentials as one executing call sees them, and the redaction of what the
//! call hands back.
//!
//! A call is given a resolver that releases only what its authorised plan
//! named, and records each release in the audit chain and in the run's ledger
//! before the tool holds the value. Whatever the call then returns, success
//! or failure, is redacted against that ledger before the model, a client or
//! the audit log sees it.

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::{ResourceRef, permission_domains};
use agentos_core::trust::DataSource;

use crate::egress::CredentialRef;
use crate::error::ToolError;
use crate::tool::{CredentialLedger, CredentialResolver, Secret, ToolOutput, ToolPlan};

/// The `{origin}/{name}` of every credential a plan asks to spend: those a
/// `network.credential` capability names, and those it declared spending under
/// its other capabilities.
pub(super) fn credentials_named(plan: &ToolPlan) -> Vec<String> {
    plan.capabilities
        .iter()
        .filter(|capability| {
            capability.domain == permission_domains::NETWORK
                && capability.action == crate::network::CREDENTIAL
        })
        .filter_map(|capability| match &capability.resource {
            Some(ResourceRef::Named { name }) => Some(name.clone()),
            _ => None,
        })
        .chain(plan.credentials.iter().map(CredentialRef::resource))
        .collect()
}

/// The run's credential store as one executing call sees it.
///
/// Answers only for the credentials the authorised plan named, so a tool that
/// planned an anonymous request, or a credential for one origin, cannot ask for
/// another once it is running: the engine was never asked about that one.
///
/// Every value it releases is recorded twice before the tool sees it: as
/// `network.credential.used` in the audit chain, and in the run's ledger, which
/// is what output is redacted against. Both are written here, at the moment of
/// release, rather than after the call returns, so that neither depends on the
/// tool's cooperation nor on the call returning at all. A request still in
/// flight when the process exits has already left its record.
#[derive(Debug)]
pub(super) struct CallCredentials {
    pub(super) store: Arc<dyn CredentialResolver>,
    pub(super) authorised: Vec<String>,
    pub(super) audit: Arc<AuditLog>,
    /// The agent, task and run the spend is recorded against.
    pub(super) spender: (AgentId, TaskId, TaskRunId),
    pub(super) tool: String,
    pub(super) released: Arc<CredentialLedger>,
}

#[async_trait::async_trait]
impl CredentialResolver for CallCredentials {
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret> {
        let resource = format!("{origin}/{name}");
        if !self.authorised.contains(&resource) {
            tracing::error!(
                credential = %resource,
                "a tool asked for a credential its plan did not name"
            );
            return None;
        }
        let secret = self.store.resolve(origin, name).await?;
        let (agent_id, task_id, run_id) = self.spender;
        let event = Event::new(AgentEvent::CredentialUsed {
            origin: origin.to_owned(),
            name: name.to_owned(),
            tool: self.tool.clone(),
        })
        .for_agent(agent_id)
        .for_task(task_id)
        .for_run(run_id);
        // Unlike every other record, this one is a condition rather than a
        // report: a spend the chain cannot show is the one thing it exists to
        // rule out, so a credential whose use cannot be recorded is not
        // released, and the call goes on without it.
        if let Err(error) = self.audit.record(event).await {
            tracing::error!(
                %error,
                credential = %resource,
                "refused to release a credential whose use could not be recorded"
            );
            return None;
        }
        self.released.record(secret.clone());
        Some(secret)
    }
}

/// A failure's text with every credential the run released replaced.
pub(super) fn redact_error(issued: &CredentialLedger, error: ToolError) -> ToolError {
    match issued.redact(&error.to_string()) {
        None => error,
        Some(message) => ToolError::Redacted {
            outcome: error.outcome(),
            message,
        },
    }
}

/// A result with every credential the run released replaced, wherever in it
/// the text could reach the model, a client or the log: the body, the URL or
/// endpoint it is labelled with, and the structured data beside it.
///
/// Scoped to what this run released rather than the whole store: it runs on
/// every call, and a run can echo only what it was given.
pub(super) fn redact_output(issued: &CredentialLedger, mut output: ToolOutput) -> ToolOutput {
    if issued.is_empty() {
        return output;
    }
    if let Some(body) = issued.redact(&output.content.body) {
        output.content.body = body;
    }
    if let DataSource::Web { url: target }
    | DataSource::Integration {
        endpoint: target, ..
    } = &mut output.content.source
        && let Some(redacted) = issued.redact(target)
    {
        *target = redacted;
    }
    if let Some(structured) = &mut output.structured {
        redact_value(issued, structured);
    }
    output
}

fn redact_value(issued: &CredentialLedger, value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(redacted) = issued.redact(text) {
                *text = redacted;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_value(issued, item);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, mut item) in std::mem::take(map) {
                redact_value(issued, &mut item);
                map.insert(issued.redact(&key).unwrap_or(key), item);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::permission::Capability;
    use agentos_core::risk::RiskLevel;
    use agentos_core::tool::ToolOutcome;

    use super::*;
    use crate::pipeline::test_support::{
        Careless, FullDisk, Spender, TOKEN, spend, spending_pipeline, spending_pipeline_with,
    };
    use crate::taint::TaintTracker;

    #[tokio::test]
    async fn a_failure_quoting_a_credential_is_redacted_before_it_is_recorded() {
        let (pipeline, sink, context) = spending_pipeline().await;
        let taint = TaintTracker::new();

        let report = spend(
            &pipeline,
            &context,
            &taint,
            serde_json::json!({"spend": true, "fail": true}),
        )
        .await;

        assert_eq!(report.outcome, ToolOutcome::Failed);
        let error = report.error.clone().unwrap();
        assert!(!error.contains(TOKEN), "{error}");
        assert!(error.contains(crate::REDACTED_CREDENTIAL), "{error}");
        assert!(!report.result.content.body.contains(TOKEN));

        let records = sink.records().await;
        let chain = serde_json::to_string(&records).unwrap();
        assert!(!chain.contains(TOKEN), "the token reached the chain");
        let failed = sink.records_of_kind("tool.execution.failed").await;
        assert_eq!(failed.len(), 1);
        assert!(
            failed[0].payload["error"]
                .as_str()
                .unwrap()
                .contains(crate::REDACTED_CREDENTIAL)
        );

        // The tool reported nothing about what it spent; the record comes from
        // what the store released, once, for the credential the plan named.
        let used = sink.records_of_kind("network.credential.used").await;
        assert_eq!(used.len(), 1);
        assert_eq!(used[0].payload["origin"], "https://a.example");
        assert_eq!(used[0].payload["name"], "deploy");
        assert_eq!(used[0].payload["tool"], "network.spend");

        // A failure that may have read is observed like a success.
        assert!(taint.is_tainted());
    }

    #[tokio::test]
    async fn a_credential_spent_earlier_in_the_run_is_redacted_from_later_calls() {
        let (pipeline, sink, context) = spending_pipeline().await;
        let taint = TaintTracker::new();

        // Before anything is spent there is nothing to redact against: the
        // scan covers what this run released, not the whole store.
        let before = spend(&pipeline, &context, &taint, serde_json::json!({})).await;
        assert!(before.result.content.body.contains(TOKEN));

        spend(
            &pipeline,
            &context,
            &taint,
            serde_json::json!({"spend": true}),
        )
        .await;

        // A later call that spends nothing but is handed the token back, by a
        // service that kept it, is redacted all the same: body, label and
        // structured data, keys included.
        let later = spend(&pipeline, &context, &taint, serde_json::json!({})).await;
        assert_eq!(later.outcome, ToolOutcome::Success);
        assert_eq!(
            later.result.content.body,
            format!("you sent {}", crate::REDACTED_CREDENTIAL)
        );
        assert!(!later.result.content.source.label().contains(TOKEN));
        let structured = later.result.structured.clone().unwrap().to_string();
        assert!(!structured.contains(TOKEN), "{structured}");
        assert_eq!(
            sink.records_of_kind("network.credential.used").await.len(),
            1,
            "only the call that spent it is recorded as spending it"
        );
    }

    #[tokio::test]
    async fn a_spend_is_on_the_chain_before_the_call_returns() {
        let (pipeline, sink, context, stalled) = spending_pipeline_with(Spender::new()).await;
        let pipeline = Arc::new(pipeline);
        let call = {
            let (pipeline, context) = (Arc::clone(&pipeline), context.clone());
            tokio::spawn(async move {
                spend(
                    &pipeline,
                    &context,
                    &TaintTracker::new(),
                    serde_json::json!({"spend": true, "stall": true}),
                )
                .await
            })
        };
        stalled.notified().await;

        // The tool holds the secret and has not returned. Whatever happens to
        // the process now, the spend is already recorded, by name.
        let used = sink.records_of_kind("network.credential.used").await;
        assert_eq!(used.len(), 1, "the spend waited for the call to return");
        assert_eq!(used[0].payload["origin"], "https://a.example");
        assert_eq!(used[0].payload["name"], "deploy");
        assert_eq!(used[0].payload["tool"], "network.spend");
        assert_eq!(used[0].run_id, Some(context.run_id));
        assert!(!sink.contains_kind("tool.execution.completed").await);
        call.abort();
    }

    #[tokio::test]
    async fn a_credential_whose_spend_cannot_be_recorded_is_not_released() {
        let audit = Arc::new(AuditLog::open(Arc::new(FullDisk)).await.unwrap());
        let plan = ToolPlan::new(RiskLevel::High, "spend").requiring(
            Capability::new("network", "credential").with_resource(ResourceRef::Named {
                name: "https://a.example/deploy".into(),
            }),
        );
        let released = Arc::new(CredentialLedger::default());
        let credentials = CallCredentials {
            store: Arc::new(Careless),
            authorised: credentials_named(&plan),
            audit,
            spender: (AgentId::new(), TaskId::new(), TaskRunId::new()),
            tool: "network.spend".into(),
            released: Arc::clone(&released),
        };
        assert!(
            credentials
                .resolve("https://a.example", "deploy")
                .await
                .is_none()
        );
        assert!(released.is_empty());
    }

    #[tokio::test]
    async fn a_call_is_given_only_the_credentials_its_plan_named() {
        let (audit, sink) = AuditLog::in_memory().await.unwrap();
        let released = Arc::new(CredentialLedger::default());
        let plan = ToolPlan::new(RiskLevel::High, "spend")
            .requiring(
                Capability::new("network", "fetch").with_resource(ResourceRef::Origin {
                    origin: "https://b.example".into(),
                }),
            )
            .requiring(Capability::new("network", "credential").with_resource(
                ResourceRef::Named {
                    name: "https://a.example/deploy".into(),
                },
            ));
        let credentials = CallCredentials {
            store: Arc::new(Careless),
            authorised: credentials_named(&plan),
            audit: Arc::new(audit),
            spender: (AgentId::new(), TaskId::new(), TaskRunId::new()),
            tool: "network.spend".into(),
            released: Arc::clone(&released),
        };
        assert!(
            credentials
                .resolve("https://b.example", "deploy")
                .await
                .is_none()
        );
        assert!(
            credentials
                .resolve("https://a.example", "admin")
                .await
                .is_none()
        );
        assert!(released.is_empty(), "a refusal releases nothing");
        assert!(!sink.contains_kind("network.credential.used").await);
        assert!(
            credentials
                .resolve("https://a.example", "deploy")
                .await
                .is_some()
        );
        assert_eq!(
            sink.records_of_kind("network.credential.used").await.len(),
            1
        );
        assert_eq!(released.len(), 1);
        assert!(!format!("{credentials:?}").contains(TOKEN));
    }

    /// An integration's call acts as the account its grant names, and spends
    /// that account's credential with no `network.credential` of its own.
    fn integration_plan() -> ToolPlan {
        ToolPlan::new(RiskLevel::Medium, "list issues in acme/widgets").requiring(
            Capability::new("github", "issues.read").with_resource(ResourceRef::Named {
                name: "acme/widgets".into(),
            }),
        )
    }

    #[tokio::test]
    async fn a_credential_a_plan_declares_is_released_and_recorded_like_a_named_one() {
        let (audit, sink) = AuditLog::in_memory().await.unwrap();
        let released = Arc::new(CredentialLedger::default());
        let plan =
            integration_plan().spending(CredentialRef::new("https://api.github.com", "work"));
        assert!(
            plan.affected_resources
                .contains(&"credential:https://api.github.com/work".to_owned()),
            "the person approving it sees whose credential it is: {:?}",
            plan.affected_resources
        );
        let credentials = CallCredentials {
            store: Arc::new(Careless),
            authorised: credentials_named(&plan),
            audit: Arc::new(audit),
            spender: (AgentId::new(), TaskId::new(), TaskRunId::new()),
            tool: "github.issues.list".into(),
            released: Arc::clone(&released),
        };
        // Another account, and the same label at another host, are refused.
        for (origin, name) in [
            ("https://api.github.com", "home"),
            ("https://ghe.example", "work"),
        ] {
            assert!(credentials.resolve(origin, name).await.is_none(), "{name}");
        }
        assert!(released.is_empty());
        assert!(
            credentials
                .resolve("https://api.github.com", "work")
                .await
                .is_some()
        );
        let used = sink.records_of_kind("network.credential.used").await;
        assert_eq!(used.len(), 1);
        assert_eq!(used[0].payload["origin"], "https://api.github.com");
        assert_eq!(used[0].payload["name"], "work");
        assert_eq!(used[0].payload["tool"], "github.issues.list");
        assert_eq!(released.len(), 1, "and it is redacted against from now on");
    }

    #[tokio::test]
    async fn a_grant_to_act_on_a_repository_releases_no_credential_by_itself() {
        // The declaration is what binds the call to an account. A plan that
        // made none gets nothing, however broad its grant.
        let (audit, sink) = AuditLog::in_memory().await.unwrap();
        let released = Arc::new(CredentialLedger::default());
        let credentials = CallCredentials {
            store: Arc::new(Careless),
            authorised: credentials_named(&integration_plan()),
            audit: Arc::new(audit),
            spender: (AgentId::new(), TaskId::new(), TaskRunId::new()),
            tool: "github.issues.list".into(),
            released: Arc::clone(&released),
        };
        assert!(
            credentials
                .resolve("https://api.github.com", "work")
                .await
                .is_none()
        );
        assert!(released.is_empty());
        assert!(!sink.contains_kind("network.credential.used").await);
    }

    #[test]
    fn an_integration_endpoint_is_redacted_like_a_url() {
        let ledger = CredentialLedger::default();
        ledger.record(Secret::new(TOKEN));
        let output = ToolOutput::text(
            DataSource::Integration {
                integration: "github".into(),
                account: "work".into(),
                endpoint: format!("/repos/acme/{TOKEN}"),
            },
            "ok",
        );
        let redacted = redact_output(&ledger, output);
        assert!(!redacted.content.source.label().contains(TOKEN));
        assert!(
            redacted
                .content
                .source
                .label()
                .ends_with(crate::REDACTED_CREDENTIAL)
        );
    }
}
