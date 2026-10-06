/**
 * The typed client for the AgentOS runtime.
 *
 * Every call the interface makes goes through here, so the set of things the
 * window can ask the runtime to do is one readable list. Components call these
 * functions; they never call `invoke` directly, and they hold no knowledge of
 * command names or argument shapes.
 *
 * The types are generated from the Rust view models — see `src/bindings` — so a
 * change on one side fails to compile on the other rather than failing at
 * runtime in front of a user.
 */

import type { AgentDetail } from "../bindings/AgentDetail";
import type { AgentSummary } from "../bindings/AgentSummary";
import type { ApprovalDecisionInput } from "../bindings/ApprovalDecisionInput";
import type { ApprovalView } from "../bindings/ApprovalView";
import type { AuditHealth } from "../bindings/AuditHealth";
import type { AuditRecordView } from "../bindings/AuditRecordView";
import type { CreateAgentInput } from "../bindings/CreateAgentInput";
import type { DashboardView } from "../bindings/DashboardView";
import type { EventView } from "../bindings/EventView";
import type { PolicyCheck } from "../bindings/PolicyCheck";
import type { PolicyView } from "../bindings/PolicyView";
import type { RunSummary } from "../bindings/RunSummary";
import type { SettingsView } from "../bindings/SettingsView";
import type { StartedTask } from "../bindings/StartedTask";
import type { TaskSummary } from "../bindings/TaskSummary";
import type { ToolUsageView } from "../bindings/ToolUsageView";
import type { ToolView } from "../bindings/ToolView";
import type { TraceView } from "../bindings/TraceView";

import { call } from "./transport";

/** Events the runtime pushes to the window. */
export const events = {
  approvalRequested: "agentos://approval-requested",
  approvalResolved: "agentos://approval-resolved",
  activity: "agentos://activity",
} as const;

export const api = {
  dashboard: () => call<DashboardView>("dashboard"),

  listAgents: () => call<AgentSummary[]>("list_agents"),
  getAgent: (name: string) => call<AgentDetail>("get_agent", { name }),
  createAgent: (input: CreateAgentInput) => call<AgentSummary>("create_agent", { input }),
  setAgentEnabled: (name: string, enabled: boolean) =>
    call<AgentSummary>("set_agent_enabled", { name, enabled }),

  checkPolicy: (document: string) => call<PolicyCheck>("check_policy", { document }),
  setPolicy: (agentId: string, document: string) =>
    call<PolicyView>("set_policy", { agentId, document }),

  listTasks: (limit?: number) => call<TaskSummary[]>("list_tasks", { limit: limit ?? null }),
  startTask: (agentId: string, objective: string) =>
    call<StartedTask>("start_task", { agentId, objective }),
  cancelRun: (runId: string) => call<boolean>("cancel_run", { runId }),
  getTrace: (runId: string) => call<TraceView>("get_trace", { runId }),
  getTaskTrace: (taskId: string) => call<TraceView>("get_task_trace", { taskId }),
  /** Every attempt at a task, oldest first. */
  listRuns: (taskId: string) => call<RunSummary[]>("list_runs", { taskId }),
  /** Start another attempt. Refused unless the latest one failed or was cancelled. */
  retryTask: (taskId: string) => call<StartedTask>("retry_task", { taskId }),

  listPendingApprovals: () => call<ApprovalView[]>("list_pending_approvals"),
  /** Answered approvals, most recently decided first; 20 unless given. */
  listRecentApprovals: (limit?: number) =>
    call<ApprovalView[]>("list_recent_approvals", { limit: limit ?? null }),
  resolveApproval: (input: ApprovalDecisionInput) => call<boolean>("resolve_approval", { input }),

  /**
   * Recent audit events, oldest first. With `securityOnly`, the runtime keeps
   * only refusals, escalations and rejections, and `limit` counts those.
   */
  activity: (limit?: number, securityOnly?: boolean) =>
    call<EventView[]>("activity", { limit: limit ?? null, securityOnly: securityOnly ?? null }),
  /** Rehash the whole chain. Deliberate and slow; Settings offers it as a button. */
  verifyAudit: () => call<string[]>("verify_audit"),
  /** The chain's health, verifying only records written since the last check. */
  auditHealth: () => call<AuditHealth>("audit_health"),
  /** One audit record in full, hashes and payload included. */
  auditRecord: (eventId: string) => call<AuditRecordView>("audit_record", { eventId }),
  /** Every audit record a run wrote, in chain order. */
  auditForRun: (runId: string) => call<AuditRecordView[]>("audit_for_run", { runId }),

  /** Each tool's calls over the last `days` days (7 unless given), busiest first. */
  toolUsage: (days?: number) => call<ToolUsageView[]>("tool_usage", { days: days ?? null }),

  listTools: () => call<ToolView[]>("list_tools"),
  settings: () => call<SettingsView>("settings"),
  setProviderKey: (provider: string, key: string) =>
    call<null>("set_provider_key", { provider, key }),
  removeProviderKey: (provider: string) => call<null>("remove_provider_key", { provider }),
};

/**
 * Turn whatever a failed command threw into something worth showing a person.
 *
 * Runtime errors arrive as strings, already written for a human; anything else
 * is a bug in the interface and says so rather than rendering `[object Object]`.
 */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return `Unexpected failure: ${JSON.stringify(error)}`;
}
