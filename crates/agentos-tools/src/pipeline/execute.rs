//! [`ToolPipeline::execute`]: one tool call, from lookup to report.
//!
//! The steps run in the order the pipeline's module documentation lists, and
//! each one that can end the call returns a report from where it stands. The
//! decisions the steps hand off to (authorisation, provenance, credentials and
//! the report itself) live in sibling modules, so this one reads as the
//! sequence and nothing else.

use std::sync::Arc;
use std::time::Instant;

use agentos_core::event::AgentEvent;
use agentos_core::ids::ToolExecutionId;
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::{ToolCall, ToolResult};
use tokio_util::sync::CancellationToken;

use super::ToolPipeline;
use super::authorise::{
    CallProbe, authorised_capabilities, build_approval_request, model_facing_reason,
};
use super::credentials::{CallCredentials, credentials_named, redact_error, redact_output};
use super::provenance::{call_may_read, provenance};
use super::report::{ExecutionReport, ReportBuilder, millis};
use crate::approval::ApprovalOutcome;
use crate::error::ToolError;
use crate::taint::TaintTracker;
use crate::tool::{CredentialResolver, ToolContext, plan_exceeds_manifest};

impl ToolPipeline {
    /// Run one tool call to completion.
    ///
    /// Never returns `Err`: every failure mode is a report the model can be told
    /// about. A tool call that cannot proceed is information, not an exception.
    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        call: &ToolCall,
        context: &ToolContext,
        taint: &TaintTracker,
        agent_name: &str,
        enabled_tools: &[String],
        cancel: &CancellationToken,
    ) -> ExecutionReport {
        let started_at = agentos_core::now();
        let clock = Instant::now();
        let execution_id = ToolExecutionId::new();
        let tainted = taint.is_tainted();

        let mut builder = ReportBuilder {
            execution_id,
            call,
            started_at,
            clock,
            tainted,
            effect: Effect::Deny,
            risk: RiskLevel::None,
            approval_id: None,
            plan: None,
            arguments: call.arguments.clone(),
        };

        // 1. Is the tool known, and offered to this agent?
        //
        // An agent asking for a tool it was not given is worth recording: it is
        // either a stale prompt or an attempt to reach further than intended.
        let Some(tool) = self.registry.get(&call.tool) else {
            self.emit(
                context,
                AgentEvent::UnknownToolRequested {
                    tool: call.tool.clone(),
                },
            )
            .await;
            return builder.failure(ToolError::UnknownTool(call.tool.clone()));
        };
        if !enabled_tools.contains(&call.tool) {
            self.emit(
                context,
                AgentEvent::UnknownToolRequested {
                    tool: call.tool.clone(),
                },
            )
            .await;
            return builder.failure(ToolError::UnknownTool(call.tool.clone()));
        }

        // 2. Validate arguments.
        let arguments = match tool.validate(&call.arguments) {
            Ok(arguments) => arguments,
            Err(error) => {
                self.emit(
                    context,
                    AgentEvent::ToolArgumentsRejected {
                        tool: call.tool.clone(),
                        error: error.to_string(),
                    },
                )
                .await;
                return builder.failure(error);
            }
        };
        builder.arguments = arguments.clone();

        // 3. Plan. Free of side effects — nothing has happened yet. Planning
        //    does look at the world, though, so a tool that may read can put
        //    what it saw into its error text, and that text is observed. There
        //    is no plan yet, so the manifest stands in for it. Nothing has
        //    been authorised either, so the tool plans without credentials.
        let plan = match tool.plan(&arguments, &context.without_credentials()).await {
            Ok(plan) => plan,
            Err(error) => {
                if call_may_read(&call.tool, &tool.metadata().required_capabilities) {
                    self.observe_failure(context, &call.tool, taint).await;
                }
                return builder.failure(error);
            }
        };
        builder.risk = plan.risk;
        builder.plan = Some(plan.clone());

        // A plan reaching past the tool's manifest means the catalogue a policy
        // author wrote rules against is stale. It is recorded, not refused: the
        // policy engine evaluates this plan next whatever the manifest says, and
        // a stale manifest must not take down a run.
        let undeclared = plan_exceeds_manifest(tool.metadata(), &plan);
        if !undeclared.is_empty() {
            tracing::error!(
                tool = %call.tool,
                ?undeclared,
                "tool planned a capability it does not declare"
            );
            self.emit(
                context,
                AgentEvent::ToolManifestExceeded {
                    tool: call.tool.clone(),
                    undeclared,
                },
            )
            .await;
        }

        // 4. Authorise every capability the plan needs. The strictest answer
        //    across all of them is the one that applies: a move that may read
        //    the source but not write the destination is not permitted.
        let decision = self.authorise(context, &call.tool, &plan, tainted).await;
        builder.effect = decision.effect;

        match decision.effect {
            Effect::Deny => {
                return builder.failure(ToolError::Denied {
                    reason: model_facing_reason(
                        &decision.reason,
                        &authorised_capabilities(&call.tool, &plan),
                    ),
                });
            }
            Effect::Ask => {
                // 5. Ask a human.
                let request = build_approval_request(
                    context, agent_name, &call.tool, &arguments, &plan, &decision, taint,
                );
                builder.approval_id = Some(request.id);

                self.emit(
                    context,
                    AgentEvent::ApprovalRequested {
                        approval_id: request.id,
                        tool: call.tool.clone(),
                        risk: plan.risk,
                    },
                )
                .await;

                let waited = Instant::now();
                let outcome = self.approvals.request(&request, cancel.clone()).await;
                match outcome {
                    ApprovalOutcome::Approved { note } => {
                        self.emit(
                            context,
                            AgentEvent::ApprovalGranted {
                                approval_id: request.id,
                                tool: call.tool.clone(),
                                waited_ms: millis(waited),
                                note,
                            },
                        )
                        .await;
                    }
                    ApprovalOutcome::Denied { note } => {
                        self.emit(
                            context,
                            AgentEvent::ApprovalDenied {
                                approval_id: request.id,
                                tool: call.tool.clone(),
                                note: note.clone(),
                                over_budget: false,
                            },
                        )
                        .await;
                        return builder.failure(ToolError::ApprovalDenied { note });
                    }
                    ApprovalOutcome::OverBudget { note } => {
                        self.emit(
                            context,
                            AgentEvent::ApprovalDenied {
                                approval_id: request.id,
                                tool: call.tool.clone(),
                                note: Some(note.clone()),
                                over_budget: true,
                            },
                        )
                        .await;
                        return builder.failure(ToolError::ApprovalDenied { note: Some(note) });
                    }
                    ApprovalOutcome::Cancelled => {
                        return builder.failure(ToolError::Cancelled);
                    }
                }
            }
            Effect::Allow => {}
        }

        if cancel.is_cancelled() {
            return builder.failure(ToolError::Cancelled);
        }

        // 6. Execute. A tool that discovers resources as it runs — a search
        //    walking a tree it was authorised to list from the root — asks the
        //    probe about each one. The probe answers from the same engine with
        //    the same request shape that authorised the call, so a narrower
        //    deny inside an allowed root binds the walk as it would bind a
        //    direct call.
        //
        //    The credential store is swapped for one that answers only for the
        //    credentials this plan named, and records what it gives out. And
        //    the call is told what it was authorised against, for a tool whose
        //    plan named something the world can change before it acts.
        let mut executing = context.clone().with_policy(Arc::new(CallProbe {
            engine: Arc::clone(&self.engine),
            tool: call.tool.clone(),
            risk: plan.risk,
            tainted,
        }));
        executing.credentials = context.credentials.as_ref().map(|store| {
            Arc::new(CallCredentials {
                store: Arc::clone(store),
                authorised: credentials_named(&plan),
                audit: Arc::clone(&self.audit),
                spender: (context.agent_id, context.task_id, context.run_id),
                tool: call.tool.clone(),
                released: context.issued_handle(),
            }) as Arc<dyn CredentialResolver>
        });
        executing.authorised = authorised_capabilities(&call.tool, &plan);
        let context = &executing;
        self.emit(
            context,
            AgentEvent::ToolExecutionStarted {
                execution_id,
                tool: call.tool.clone(),
                arguments: arguments.clone(),
            },
        )
        .await;

        let limit = tool.time_limit(context);
        let running = tool.execute(arguments, context, cancel.clone());
        let execution = match limit {
            Some(limit) => tokio::time::timeout(limit, running).await,
            None => Ok(running.await),
        };

        let output = match execution {
            Err(_elapsed) => {
                let error = ToolError::TimedOut {
                    tool: call.tool.clone(),
                    seconds: limit.unwrap_or_default().as_secs(),
                };
                self.emit(
                    context,
                    AgentEvent::ToolExecutionFailed {
                        execution_id,
                        tool: call.tool.clone(),
                        duration_ms: millis(clock),
                        error: error.to_string(),
                    },
                )
                .await;
                return builder.failure(error);
            }
            Ok(Err(error)) => {
                // The text is hashed into the chain below, so it is redacted
                // before it is recorded, not after.
                let error = redact_error(context.issued(), error);
                self.emit(
                    context,
                    AgentEvent::ToolExecutionFailed {
                        execution_id,
                        tool: call.tool.clone(),
                        duration_ms: millis(clock),
                        error: error.to_string(),
                    },
                )
                .await;
                // A tool that ran and failed hands the model text it composed
                // from what it met: a response body, a parse error quoting the
                // input, stderr. That reaches the model as surely as a success,
                // so a call that may read is judged the same way either way. A
                // write that fails reports the operating system's complaint
                // about the model's own path, and leaves the run as it was.
                if call_may_read(&call.tool, &plan.capabilities) {
                    self.observe_failure(context, &call.tool, taint).await;
                }
                return builder.failure(error);
            }
            Ok(Ok(output)) => redact_output(context.issued(), output),
        };

        // 7. Everything a tool returns is untrusted. If it came from outside,
        //    the run is now tainted and every later decision is stricter.
        //    "From outside" is read from the provenance of the bytes and from
        //    the plan that was authorised, never from what the tool says it
        //    returns: a flag a tool sets about itself cannot switch this off.
        let source = provenance(&call.tool, &plan, &output.content.source);
        if taint.observe(&source) {
            self.emit(
                context,
                AgentEvent::TaintRaised {
                    source,
                    tool: call.tool.clone(),
                },
            )
            .await;
        }

        let content = output.content.truncated(context.max_output_bytes);
        let output_bytes = content.len();

        self.emit(
            context,
            AgentEvent::ToolExecutionCompleted {
                execution_id,
                tool: call.tool.clone(),
                duration_ms: millis(clock),
                success: true,
                output_bytes,
            },
        )
        .await;

        // 8. Images are subject to the same budget discipline as text. A tool
        //    that hands back more captures than the run allows has the surplus
        //    dropped here rather than at the provider, where the operator would
        //    never see it happen.
        let mut images = output.images;
        let surplus = images.len().saturating_sub(context.max_images);
        if surplus > 0 {
            images.truncate(context.max_images);
            tracing::warn!(
                tool = %call.tool,
                dropped = surplus,
                kept = context.max_images,
                "dropped images over the run's per-call limit"
            );
        }
        let image_bytes: usize = images
            .iter()
            .map(agentos_core::trust::UntrustedImage::len)
            .sum();
        for image in &images {
            if taint.observe(&image.source) {
                self.emit(
                    context,
                    AgentEvent::TaintRaised {
                        source: image.source.clone(),
                        tool: call.tool.clone(),
                    },
                )
                .await;
            }
        }

        let mut result = ToolResult::success(&call.id, &call.tool, content).with_images(images);
        result.structured = output.structured;
        builder.success(result, output_bytes + image_bytes)
    }
}

#[cfg(test)]
mod tests {
    use agentos_audit::AuditLog;
    use agentos_core::permission::Capability;
    use agentos_core::tool::ToolOutcome;
    use agentos_core::trust::DataSource;

    use super::*;
    use crate::pipeline::test_support::{spend, spending_pipeline};
    use crate::tool::{ToolOutput, ToolPlan, ToolRegistry};

    #[tokio::test]
    async fn a_call_is_told_what_it_was_authorised_against() {
        let (pipeline, _sink, context) = spending_pipeline().await;
        assert!(context.authorised.is_empty());
        let report = spend(
            &pipeline,
            &context,
            &TaintTracker::new(),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(report.outcome, ToolOutcome::Success, "{:?}", report.error);
        assert_eq!(
            report.result.structured.unwrap()["authorised"],
            serde_json::json!(["network.credential on name:https://a.example/deploy"])
        );
    }

    /// Holds its call open until released, saying when it has started, and
    /// declares no time limit of its own when `unlimited`.
    #[derive(Debug)]
    struct Container {
        metadata: agentos_core::tool::ToolMetadata,
        unlimited: bool,
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl crate::Tool for Container {
        fn metadata(&self) -> &agentos_core::tool::ToolMetadata {
            &self.metadata
        }

        fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(arguments.clone())
        }

        async fn plan(
            &self,
            _arguments: &serde_json::Value,
            _context: &ToolContext,
        ) -> Result<ToolPlan, ToolError> {
            Ok(ToolPlan::new(RiskLevel::Low, "contain")
                .requiring(Capability::new("runtime", "delegate")))
        }

        fn time_limit(&self, context: &ToolContext) -> Option<std::time::Duration> {
            (!self.unlimited).then_some(context.timeout)
        }

        async fn execute(
            &self,
            _arguments: serde_json::Value,
            _context: &ToolContext,
            _cancel: CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(ToolOutput::text(
                DataSource::Tool {
                    tool: "contain".into(),
                },
                "done",
            ))
        }
    }

    /// Run one call to a [`Container`] under a one-second limit, letting an
    /// hour of the paused clock pass while it is held open.
    async fn hold_for_an_hour(unlimited: bool) -> ExecutionReport {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(Container {
            metadata: crate::tool::metadata_for::<serde_json::Value>(
                "runtime.contain",
                "holds a call open",
                RiskLevel::Low,
                vec![Capability::new("runtime", "delegate")],
                false,
            ),
            unlimited,
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        }));
        let (audit, _sink) = AuditLog::in_memory().await.unwrap();
        let policy = agentos_permissions::Policy::deny_all("t").with_rule(
            agentos_permissions::policy::PolicyRule::new(
                "contain",
                "runtime",
                "delegate",
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
        .with_timeout(std::time::Duration::from_secs(1));

        let call = tokio::spawn(async move {
            pipeline
                .execute(
                    &ToolCall::new("1", "runtime.contain", serde_json::json!({})),
                    &context,
                    &TaintTracker::new(),
                    "agent",
                    &["runtime.contain".to_owned()],
                    &CancellationToken::new(),
                )
                .await
        });
        started.notified().await;
        // The clock is paused, so this hour passes as soon as nothing else
        // can run: any limit the pipeline set on the call has fired by then.
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        release.notify_one();
        call.await.unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_tool_that_declares_no_time_limit_is_not_cut_off() {
        let report = hold_for_an_hour(true).await;
        assert!(report.is_success(), "{:?}", report.error);
    }

    #[tokio::test(start_paused = true)]
    async fn every_other_tool_is_held_to_the_context_timeout() {
        let report = hold_for_an_hour(false).await;
        assert_eq!(report.outcome, ToolOutcome::TimedOut);
    }
}
