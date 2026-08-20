//! The tool execution pipeline.
//!
//! Every tool call the model makes goes through here, in a fixed order:
//!
//! ```text
//! is the tool known and enabled?
//!         ↓
//! validate arguments against the schema
//!         ↓
//! plan: what capabilities, what risk, what resources
//!         ↓
//! evaluate each capability against the policy   ← the model has no say here
//!         ↓
//! deny        → return a refusal to the model
//! ask         → put an approval to a human, wait
//! allow       → proceed
//!         ↓
//! execute with a timeout and a cancellation token
//!         ↓
//! capture output as untrusted, raise taint if external
//!         ↓
//! emit audit events at every step, return a report
//! ```
//!
//! Two properties are worth stating plainly:
//!
//! * **Nothing executes before authorisation.** `plan` is pure and side-effect
//!   free; the first side effect a tool has is inside `execute`, which is only
//!   reached after the policy engine and, where required, a human.
//! * **A refusal is a result, not a crash.** Denials come back to the model as
//!   ordinary tool output so it can re-plan. What it must never get is the
//!   ability to argue its way past the decision, and it cannot: the decision was
//!   made from the policy, not from anything the model wrote.

use std::sync::Arc;
use std::time::Instant;

use agentos_audit::AuditLog;
use agentos_core::Timestamp;
use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{ApprovalId, ToolExecutionId};
use agentos_core::permission::{Effect, PermissionDecision, PermissionRequest};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::{ToolCall, ToolOutcome, ToolResult};
use agentos_core::trust::{DataSource, UntrustedContent};
use agentos_permissions::PermissionEngine;
use tokio_util::sync::CancellationToken;

use crate::approval::{ApprovalGate, ApprovalOutcome};
use crate::error::ToolError;
use crate::taint::TaintTracker;
use crate::tool::{ToolContext, ToolPlan, ToolRegistry};

/// Everything that happened during one tool invocation.
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    /// Identity of this execution.
    pub execution_id: ToolExecutionId,
    /// The tool.
    pub tool: String,
    /// The provider's call identifier.
    pub call_id: String,
    /// Validated arguments, or the raw ones if validation failed.
    pub arguments: serde_json::Value,
    /// How it ended.
    pub outcome: ToolOutcome,
    /// The permission effect that applied.
    pub effect: Effect,
    /// Assessed risk.
    pub risk: RiskLevel,
    /// Whether the run was tainted at the time of the call.
    pub tainted: bool,
    /// The approval that gated it, if any.
    pub approval_id: Option<ApprovalId>,
    /// Bytes returned to the model.
    pub output_bytes: u64,
    /// Error text, when it failed.
    pub error: Option<String>,
    /// How long the whole pipeline took.
    pub duration_ms: u64,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished.
    pub completed_at: Timestamp,
    /// The plan, when one was produced.
    pub plan: Option<ToolPlan>,
    /// What goes back to the model.
    pub result: ToolResult,
}

impl ExecutionReport {
    /// Whether the tool actually ran and succeeded.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self.outcome, ToolOutcome::Success)
    }
}

/// Runs tool calls through validation, authorisation, approval and execution.
#[derive(Debug, Clone)]
pub struct ToolPipeline {
    registry: Arc<ToolRegistry>,
    engine: Arc<dyn PermissionEngine>,
    approvals: Arc<dyn ApprovalGate>,
    audit: Arc<AuditLog>,
}

impl ToolPipeline {
    /// Assemble a pipeline.
    #[must_use]
    pub const fn new(
        registry: Arc<ToolRegistry>,
        engine: Arc<dyn PermissionEngine>,
        approvals: Arc<dyn ApprovalGate>,
        audit: Arc<AuditLog>,
    ) -> Self {
        Self {
            registry,
            engine,
            approvals,
            audit,
        }
    }

    /// The registry this pipeline draws from.
    #[must_use]
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

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

        // 3. Plan. Pure — nothing has happened yet.
        let plan = match tool.plan(&arguments, context).await {
            Ok(plan) => plan,
            Err(error) => return builder.failure(error),
        };
        builder.risk = plan.risk;
        builder.plan = Some(plan.clone());

        // 4. Authorise every capability the plan needs. The strictest answer
        //    across all of them is the one that applies: a move that may read
        //    the source but not write the destination is not permitted.
        let decision = self.authorise(context, &call.tool, &plan, tainted).await;
        builder.effect = decision.effect;

        match decision.effect {
            Effect::Deny => {
                return builder.failure(ToolError::Denied {
                    reason: decision.reason,
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
                    ApprovalOutcome::Approved => {
                        self.emit(
                            context,
                            AgentEvent::ApprovalGranted {
                                approval_id: request.id,
                                tool: call.tool.clone(),
                                waited_ms: millis(waited),
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
                            },
                        )
                        .await;
                        return builder.failure(ToolError::ApprovalDenied { note });
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

        // 6. Execute.
        self.emit(
            context,
            AgentEvent::ToolExecutionStarted {
                execution_id,
                tool: call.tool.clone(),
                arguments: arguments.clone(),
            },
        )
        .await;

        let execution = tokio::time::timeout(
            context.timeout,
            tool.execute(arguments, context, cancel.clone()),
        )
        .await;

        let output = match execution {
            Err(_elapsed) => {
                let error = ToolError::TimedOut {
                    tool: call.tool.clone(),
                    seconds: context.timeout.as_secs(),
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
            Ok(Ok(output)) => output,
        };

        // 7. Everything a tool returns is untrusted. If it came from outside,
        //    the run is now tainted and every later decision is stricter.
        if tool.metadata().returns_untrusted_data && taint.observe(&output.content.source) {
            self.emit(
                context,
                AgentEvent::TaintRaised {
                    source: output.content.source.clone(),
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

        let mut result = ToolResult::success(&call.id, &call.tool, content);
        result.structured = output.structured;
        builder.success(result, output_bytes)
    }

    /// Evaluate every capability in a plan and combine the answers.
    async fn authorise(
        &self,
        context: &ToolContext,
        tool: &str,
        plan: &ToolPlan,
        tainted: bool,
    ) -> PermissionDecision {
        // A plan that needs no capability is still authorised, against an
        // unscoped capability derived from the tool name. A tool that forgets to
        // declare what it touches must not thereby become unrestricted.
        let capabilities = if plan.capabilities.is_empty() {
            vec![capability_from_tool_name(tool)]
        } else {
            plan.capabilities.clone()
        };

        let mut combined: Option<PermissionDecision> = None;

        for capability in capabilities {
            let request =
                PermissionRequest::new(tool, capability.clone(), plan.risk).tainted(tainted);

            self.emit(
                context,
                AgentEvent::PermissionRequested {
                    tool: tool.to_owned(),
                    capability: capability.clone(),
                    risk: plan.risk,
                    tainted,
                },
            )
            .await;

            let decision = self.engine.evaluate(&request);

            if decision.was_escalated_by_taint() {
                self.emit(
                    context,
                    AgentEvent::PermissionEscalatedByTaint {
                        tool: tool.to_owned(),
                        original: decision.effect_before_taint,
                        escalated: decision.effect,
                    },
                )
                .await;
            }

            match decision.effect {
                Effect::Deny => {
                    self.emit(
                        context,
                        AgentEvent::PermissionDenied {
                            tool: tool.to_owned(),
                            capability,
                            reason: decision.reason.clone(),
                            matched_rule: decision.matched_rule.clone(),
                        },
                    )
                    .await;
                }
                Effect::Allow | Effect::Ask => {
                    self.emit(
                        context,
                        AgentEvent::PermissionGranted {
                            tool: tool.to_owned(),
                            capability,
                            matched_rule: decision.matched_rule.clone(),
                        },
                    )
                    .await;
                }
            }

            combined = Some(match combined {
                None => decision,
                Some(existing)
                    if decision.effect.stricter(existing.effect) == decision.effect
                        && decision.effect != existing.effect =>
                {
                    decision
                }
                Some(existing) => existing,
            });
        }

        combined.unwrap_or_else(|| {
            PermissionDecision::new(Effect::Deny, "no capability could be evaluated")
        })
    }

    async fn emit(&self, context: &ToolContext, payload: AgentEvent) {
        let event = Event::new(payload)
            .for_agent(context.agent_id)
            .for_task(context.task_id)
            .for_run(context.run_id);
        if let Err(error) = self.audit.record(event).await {
            // Losing an audit record is serious but must not abort the run: the
            // alternative is that a failing disk silently stops all work.
            tracing::error!(%error, "failed to record audit event");
        }
    }
}

/// Turn `domain.action` into a capability for a tool that declared none.
fn capability_from_tool_name(tool: &str) -> agentos_core::permission::Capability {
    let (domain, action) = tool.split_once('.').unwrap_or((tool, "invoke"));
    agentos_core::permission::Capability::new(domain, action)
}

fn build_approval_request(
    context: &ToolContext,
    agent_name: &str,
    tool: &str,
    arguments: &serde_json::Value,
    plan: &ToolPlan,
    decision: &PermissionDecision,
    taint: &TaintTracker,
) -> ApprovalRequest {
    let capability = plan
        .capabilities
        .first()
        .cloned()
        .unwrap_or_else(|| capability_from_tool_name(tool));

    let mut explanation = plan.summary.clone();
    if taint.is_tainted() {
        // The single most decision-relevant fact for a human: this agent has
        // been reading things it did not write.
        let sources = taint
            .sources()
            .iter()
            .map(agentos_core::trust::DataSource::label)
            .collect::<Vec<_>>()
            .join(", ");
        explanation.push_str(&format!(
            "\n\nThis agent has read untrusted data during this run ({sources}). \
             Anything it proposes may have been influenced by that content."
        ));
    }

    ApprovalRequest {
        id: ApprovalId::new(),
        agent_id: context.agent_id,
        agent_name: agent_name.to_owned(),
        task_id: context.task_id,
        run_id: context.run_id,
        tool: tool.to_owned(),
        arguments: arguments.clone(),
        capability,
        risk: plan.risk,
        reason: decision.reason.clone(),
        explanation,
        affected_resources: plan.affected_resources.clone(),
        tainted: taint.is_tainted(),
        status: ApprovalStatus::Pending,
        requested_at: agentos_core::now(),
        decided_at: None,
        decision_note: None,
    }
}

fn millis(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Accumulates the fields of an [`ExecutionReport`] as the pipeline progresses.
struct ReportBuilder<'a> {
    execution_id: ToolExecutionId,
    call: &'a ToolCall,
    started_at: Timestamp,
    clock: Instant,
    tainted: bool,
    effect: Effect,
    risk: RiskLevel,
    approval_id: Option<ApprovalId>,
    plan: Option<ToolPlan>,
    arguments: serde_json::Value,
}

impl ReportBuilder<'_> {
    fn failure(self, error: ToolError) -> ExecutionReport {
        let outcome = error.outcome();
        let message = error.to_string();
        // The refusal text is itself untrusted: it can embed a path or a URL the
        // model chose, and the model should not read its own strings back as
        // instructions.
        let content = UntrustedContent::new(
            DataSource::Tool {
                tool: self.call.tool.clone(),
            },
            &message,
        );
        let result = ToolResult {
            call_id: self.call.id.clone(),
            tool: self.call.tool.clone(),
            outcome,
            content,
            structured: None,
        };
        self.finish(outcome, result, message.len(), Some(message))
    }

    fn success(self, result: ToolResult, output_bytes: usize) -> ExecutionReport {
        self.finish(ToolOutcome::Success, result, output_bytes, None)
    }

    fn finish(
        self,
        outcome: ToolOutcome,
        result: ToolResult,
        output_bytes: usize,
        error: Option<String>,
    ) -> ExecutionReport {
        ExecutionReport {
            execution_id: self.execution_id,
            tool: self.call.tool.clone(),
            call_id: self.call.id.clone(),
            arguments: self.arguments,
            outcome,
            effect: self.effect,
            risk: self.risk,
            tainted: self.tainted,
            approval_id: self.approval_id,
            output_bytes: output_bytes as u64,
            error,
            duration_ms: millis(self.clock),
            started_at: self.started_at,
            completed_at: agentos_core::now(),
            plan: self.plan,
            result,
        }
    }
}
