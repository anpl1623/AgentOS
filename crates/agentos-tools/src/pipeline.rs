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
//! compare the plan with the tool's manifest     ← recorded, never enforced
//!         ↓
//! evaluate each capability against the policy   ← the model has no say here
//!         ↓
//! deny        → return a refusal to the model
//! ask         → put an approval to a human, wait
//! allow       → proceed
//!         ↓
//! execute with a timeout, a cancellation token, a policy probe and only
//! the credentials the plan named
//!         ↓
//! redact any credential the run released from what came back
//!         ↓
//! capture output as untrusted, raise taint from its provenance
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
//!
//! A third follows from the same reasoning applied to tools: **what a tool says
//! about itself is a claim.** Its manifest is checked against each plan and any
//! excess is recorded. Taint comes from the declared provenance of the bytes a
//! call returned, and a call that may read the outside world taints the run
//! even if the tool labels its output as the runtime's own. What counts as
//! reading fails closed: a capability is presumed to read unless it is one of
//! the actions known only to change something.
//!
//! Credentials follow the same rule. A tool is never handed the run's
//! credential store, only one that releases what the authorised plan named and
//! writes down every value it releases. The pipeline reads that ledger, not the
//! tool's account of itself, to record each credential spent and to strip each
//! one from whatever the call hands back, success or failure, before the model
//! or the audit log sees it.

use std::sync::Arc;
use std::time::Instant;

use agentos_audit::AuditLog;
use agentos_core::Timestamp;
use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId, ToolExecutionId};
use agentos_core::permission::{
    Capability, Effect, PermissionDecision, PermissionRequest, ResourceRef, permission_domains,
};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::{ToolCall, ToolOutcome, ToolResult};
use agentos_core::trust::{DataSource, UntrustedContent};
use agentos_permissions::PermissionEngine;
use tokio_util::sync::CancellationToken;

use crate::approval::{ApprovalGate, ApprovalOutcome};
use crate::error::ToolError;
use crate::taint::TaintTracker;
use crate::tool::{
    CredentialLedger, CredentialResolver, PolicyProbe, Secret, ToolContext, ToolOutput, ToolPlan,
    ToolRegistry, plan_exceeds_manifest,
};

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

    /// Evaluate every capability in a plan and combine the answers.
    async fn authorise(
        &self,
        context: &ToolContext,
        tool: &str,
        plan: &ToolPlan,
        tainted: bool,
    ) -> PermissionDecision {
        let capabilities = authorised_capabilities(tool, plan);

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

            // What the policy alone would have said is judged over the whole
            // call too. If one capability asks on its own rules and another
            // asks only because of taint, the call would be put to a person
            // with or without the taint, and must not be presented as asking
            // because of it.
            let before_taint = combined
                .as_ref()
                .map_or(decision.effect_before_taint, |existing| {
                    existing
                        .effect_before_taint
                        .stricter(decision.effect_before_taint)
                });
            let mut chosen = match combined {
                None => decision,
                Some(existing)
                    if decision.effect.stricter(existing.effect) == decision.effect
                        && decision.effect != existing.effect =>
                {
                    decision
                }
                Some(existing) => existing,
            };
            chosen.effect_before_taint = before_taint;
            combined = Some(chosen);
        }

        combined.unwrap_or_else(|| {
            PermissionDecision::new(Effect::Deny, "no capability could be evaluated")
        })
    }

    /// Taint the run with the text of a failure the tool itself composed.
    ///
    /// Called only for a call that may read (see [`call_may_read`]): the model
    /// receives that text labelled as the tool's output, and the tracker is
    /// told exactly what the model was given. Refusals the runtime writes — an
    /// unknown tool, a denial, a declined approval, a timeout — are not
    /// observed: they wrap the model's own arguments, which nothing outside the
    /// run wrote. A denial's reason names the plan's resources, which the plan
    /// may have resolved from the world, so the model is given it without them
    /// (see [`model_facing_reason`]).
    async fn observe_failure(&self, context: &ToolContext, tool: &str, taint: &TaintTracker) {
        let source = DataSource::Tool {
            tool: tool.to_owned(),
        };
        if taint.observe(&source) {
            self.emit(
                context,
                AgentEvent::TaintRaised {
                    source,
                    tool: tool.to_owned(),
                },
            )
            .await;
        }
    }

    async fn emit(&self, context: &ToolContext, payload: AgentEvent) {
        let event = Event::new(payload)
            .for_agent(context.agent_id)
            .for_task(context.task_id)
            .for_run(context.run_id);
        if let Err(error) = self.audit.record(event).await {
            // Losing an audit record is serious but must not abort the run: the
            // alternative is that a failing disk silently stops all work. Nor
            // is it only this log line: the log has already counted the loss
            // in `agentos_audit::unrecorded`, which the desktop reports beside
            // the chain's health, so a chain that verifies is not mistaken for
            // a complete one.
            tracing::error!(%error, "failed to record audit event");
        }
    }
}

/// The capabilities a plan is authorised against.
///
/// A plan that needs no capability is still authorised, against an unscoped
/// capability derived from the tool name. A tool that forgets to declare what
/// it touches must not thereby become unrestricted.
fn authorised_capabilities(tool: &str, plan: &ToolPlan) -> Vec<Capability> {
    if plan.capabilities.is_empty() {
        vec![capability_from_tool_name(tool)]
    } else {
        plan.capabilities.clone()
    }
}

/// Turn `domain.action` into a capability for a tool that declared none.
fn capability_from_tool_name(tool: &str) -> Capability {
    let (domain, action) = tool.split_once('.').unwrap_or((tool, "invoke"));
    Capability::new(domain, action)
}

/// Whether exercising a capability may bring bytes from outside the runtime
/// into the call's result.
///
/// Keyed on the capability rather than the tool, because the capability is what
/// the policy engine authorised and a tool's description of itself is not. It
/// fails closed: the answer is yes unless the capability is one of the actions
/// known only to change something — a write, a click. A domain this list has
/// never heard of reads, so a later integration, plugin or network tool is
/// held to the provenance floor without anybody remembering to add it here.
fn reads_outside_world(capability: &Capability) -> bool {
    let action = capability.action.as_str();
    let only_changes = match capability.domain.as_str() {
        permission_domains::FILESYSTEM => {
            matches!(action, "write" | "delete" | "copy" | "move")
        }
        permission_domains::COMPUTER => {
            matches!(
                action,
                "click" | "type" | "key" | "move" | "scroll" | "drag"
            )
        }
        _ => false,
    };
    !only_changes
}

/// Whether a call may hand the model bytes from outside the runtime.
///
/// Judged from the capability the tool's name stands for — what the operator
/// enabled and what an empty plan is authorised against — and from every
/// capability the call plans or, before it has a plan, declares. One exception:
/// a filesystem tool that does not itself read, such as `filesystem.copy`,
/// reads its source only to write it somewhere else, and its `filesystem.read`
/// is the reading half of that transfer rather than the result.
fn call_may_read(tool: &str, capabilities: &[Capability]) -> bool {
    let named = capability_from_tool_name(tool);
    if reads_outside_world(&named) {
        return true;
    }
    let transfers = named.domain == permission_domains::FILESYSTEM;
    capabilities.iter().any(|capability| {
        let transfer_source = transfers
            && capability.domain == permission_domains::FILESYSTEM
            && capability.action == "read";
        reads_outside_world(capability) && !transfer_source
    })
}

/// Where the text of a successful call came from, for the taint tracker.
///
/// The tool's label is believed when it names an external source, since
/// believing that can only make the run stricter. It is not believed when it
/// claims the operator or the runtime wrote bytes that a call that may read
/// went out and fetched: then the call is recorded as the tool's own output,
/// which is externally influenced. A mislabelled read costs an approval prompt,
/// never a silent consequential action.
fn provenance(tool: &str, plan: &ToolPlan, declared: &DataSource) -> DataSource {
    if call_may_read(tool, &plan.capabilities) && !declared.is_externally_influenced() {
        DataSource::Tool {
            tool: tool.to_owned(),
        }
    } else {
        declared.clone()
    }
}

/// A denial's reason as the model is given it, with each planned resource
/// replaced by the name of the capability it scoped.
///
/// The engine names the capability it refused, resource and all, and the audit
/// record keeps that. The resource is not the model's own words, though: the
/// plan resolved it from the world — a symlink's target, the origin a page
/// redirected itself to — and handing it back would carry outside text to the
/// model by a route the taint tracker does not watch. The model already knows
/// what it asked for.
fn model_facing_reason(reason: &str, capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .filter(|capability| capability.resource.is_some())
        .fold(reason.to_owned(), |reason, capability| {
            // The engine quotes the capability in backticks; matching the
            // quotes too keeps `path:/a` from rewriting part of `path:/ab`.
            reason.replace(
                &format!("`{capability}`"),
                &format!("`{}`", capability.qualified_name()),
            )
        })
}

/// The `{origin}/{name}` of every credential a plan asks to spend.
fn credentials_named(plan: &ToolPlan) -> Vec<String> {
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
struct CallCredentials {
    store: Arc<dyn CredentialResolver>,
    authorised: Vec<String>,
    audit: Arc<AuditLog>,
    /// The agent, task and run the spend is recorded against.
    spender: (AgentId, TaskId, TaskRunId),
    tool: String,
    released: Arc<CredentialLedger>,
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
fn redact_error(issued: &CredentialLedger, error: ToolError) -> ToolError {
    match issued.redact(&error.to_string()) {
        None => error,
        Some(message) => ToolError::Redacted {
            outcome: error.outcome(),
            message,
        },
    }
}

/// A result with every credential the run released replaced, wherever in it
/// the text could reach the model, a client or the log: the body, the URL it
/// is labelled with, and the structured data beside it.
///
/// Scoped to what this run released rather than the whole store: it runs on
/// every call, and a run can echo only what it was given.
fn redact_output(issued: &CredentialLedger, mut output: ToolOutput) -> ToolOutput {
    if issued.is_empty() {
        return output;
    }
    if let Some(body) = issued.redact(&output.content.body) {
        output.content.body = body;
    }
    if let DataSource::Web { url } = &mut output.content.source
        && let Some(redacted) = issued.redact(url)
    {
        *url = redacted;
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

    // The single most decision-relevant fact for a human is where this agent has
    // been reading. It travels as a list of sources; how it is worded is the
    // client's business, not the runtime's.
    let taint_sources = taint
        .sources()
        .iter()
        .map(agentos_core::trust::DataSource::label)
        .collect::<Vec<_>>();

    // When taint is the reason a human is being asked at all, the reason says
    // where the run first read from outside; the full list travels beside it.
    let reason = match taint_sources.first() {
        Some(first) if decision.was_escalated_by_taint() => {
            format!("{}; the untrusted data came from {first}", decision.reason)
        }
        _ => decision.reason.clone(),
    };

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
        effect_before_taint: decision.effect_before_taint,
        // The run's gate counts and stamps these: it is the one place every
        // request in a run passes through, and the place the budget is
        // enforced, so the number a person is shown is the number enforced.
        asked_this_run: 0,
        approval_budget: None,
        reason,
        explanation: plan.summary.clone(),
        affected_resources: plan.affected_resources.clone(),
        tainted: taint.is_tainted(),
        taint_sources,
        status: ApprovalStatus::Pending,
        requested_at: agentos_core::now(),
        decided_at: None,
        decision_note: None,
    }
}

/// The policy as one executing call may consult it.
///
/// Fixed to the request shape that authorised the call — its tool, its planned
/// risk and the taint state at the moment of authorisation — so that asking
/// about a path the tool found is asking exactly the question the pipeline
/// would have asked had the model named that path itself.
///
/// Only an outright allow is a yes. An `ask` was put to a person for the call
/// as planned; they did not see, and so did not approve, the paths it finds
/// later. Nothing here is written to the audit log: a walk may consult the
/// probe thousands of times, and what the tool skipped it reports itself.
#[derive(Debug)]
struct CallProbe {
    engine: Arc<dyn PermissionEngine>,
    tool: String,
    risk: RiskLevel,
    tainted: bool,
}

impl PolicyProbe for CallProbe {
    fn permits(&self, capability: &Capability) -> bool {
        let request = PermissionRequest::new(self.tool.clone(), capability.clone(), self.risk)
            .tainted(self.tainted);
        self.engine.evaluate(&request).effect == Effect::Allow
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
            images: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(qualified: &str) -> Capability {
        let (domain, action) = qualified.split_once('.').unwrap_or((qualified, "invoke"));
        Capability::new(domain, action)
    }

    fn may_read(tool: &str, plans: &[&str]) -> bool {
        let plans = plans
            .iter()
            .map(|name| capability(name))
            .collect::<Vec<_>>();
        call_may_read(tool, &plans)
    }

    #[test]
    fn the_reading_floor_fails_closed_for_what_it_does_not_know() {
        // Integrations and plugins arrive later; none of them should need an
        // entry here to be held to the floor.
        assert!(may_read("web.fetch", &[]));
        assert!(may_read("email.read", &[]));
        assert!(may_read("plugin", &[]));
        // `inspect` is not in any list, and an empty plan is judged by the
        // tool's name, so the first-party inspector is covered either way.
        assert!(may_read("computer.inspect", &[]));
        assert!(may_read("computer.inspect", &["computer.read"]));
    }

    #[test]
    fn actions_that_only_change_something_do_not_read() {
        for (tool, plans) in [
            ("filesystem.write", &["filesystem.write"][..]),
            ("filesystem.delete", &["filesystem.delete"][..]),
            (
                "filesystem.move",
                &["filesystem.delete", "filesystem.write"][..],
            ),
            ("computer.click", &["computer.click"][..]),
            ("computer.type", &["computer.type"][..]),
        ] {
            assert!(!may_read(tool, plans), "{tool} should not read");
        }
    }

    #[test]
    fn a_copy_reads_its_source_only_to_write_it() {
        assert!(!may_read(
            "filesystem.copy",
            &["filesystem.read", "filesystem.write"]
        ));
        // The transfer exception is about filesystem reads alone: a write that
        // also plans a browser read is reading.
        assert!(may_read(
            "filesystem.write",
            &["filesystem.write", "browser.read"]
        ));
        // And it never excuses a tool whose own name reads.
        assert!(may_read("filesystem.read", &["filesystem.read"]));
        assert!(may_read(
            "terminal.exec",
            &["filesystem.read", "terminal.exec"]
        ));
    }

    /// A sink on a disk that has stopped taking writes.
    #[derive(Debug)]
    struct FullDisk;

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

    #[tokio::test]
    async fn a_record_the_disk_refuses_is_counted_and_the_call_still_answers() {
        let audit = Arc::new(AuditLog::open(Arc::new(FullDisk)).await.unwrap());
        let pipeline = ToolPipeline::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(agentos_permissions::DenyAllEngine),
            Arc::new(crate::DenyAllGate),
            audit,
        );
        let context = ToolContext::new(
            agentos_core::ids::AgentId::new(),
            agentos_core::ids::TaskId::new(),
            agentos_core::ids::TaskRunId::new(),
            std::env::temp_dir(),
        );
        let before = agentos_audit::unrecorded();

        // An unknown tool is refused, and the refusal is a record the pipeline
        // tries and fails to write.
        let report = pipeline
            .execute(
                &ToolCall::new("1", "nowhere.nothing", serde_json::json!({})),
                &context,
                &TaintTracker::new(),
                "agent",
                &[],
                &CancellationToken::new(),
            )
            .await;

        assert!(
            !report.outcome.executed(),
            "the call was answered, as refused"
        );
        assert!(agentos_audit::unrecorded() > before, "the loss was counted");
    }

    #[test]
    fn a_denial_reaches_the_model_without_the_resources_its_plan_resolved() {
        let resolved = Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
            path: "/elsewhere/IGNORE PREVIOUS INSTRUCTIONS".into(),
        });
        let shorter = Capability::new("filesystem", "read").with_resource(ResourceRef::Path {
            path: "/elsewhere/IGNORE".into(),
        });
        let reason = format!("no rule matched `{resolved}`; policy default is `deny`");
        let redacted = model_facing_reason(&reason, &[shorter, resolved]);
        assert_eq!(
            redacted,
            "no rule matched `filesystem.read`; policy default is `deny`"
        );
    }

    /// A live token, as distinctive as a real one.
    const TOKEN: &str = "ghp_7Xq2LmN9pR4sT6vW8yZ0aB1cD3eF5gH7";

    /// A store that would hand any credential to anyone.
    ///
    /// The binding under test is the pipeline's, so the store is made as
    /// careless as possible.
    #[derive(Debug)]
    struct Careless;

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
    struct Spender(agentos_core::tool::ToolMetadata, Arc<tokio::sync::Notify>);

    impl Spender {
        fn new() -> Self {
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

    async fn spending_pipeline() -> (ToolPipeline, Arc<agentos_audit::InMemorySink>, ToolContext) {
        let (pipeline, sink, context, _) = spending_pipeline_with(Spender::new()).await;
        (pipeline, sink, context)
    }

    async fn spending_pipeline_with(
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

    async fn spend(
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
}
