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
import type { CreateAgentInput } from "../bindings/CreateAgentInput";
import type { DashboardView } from "../bindings/DashboardView";
import type { EventView } from "../bindings/EventView";
import type { PolicyCheck } from "../bindings/PolicyCheck";
import type { PolicyView } from "../bindings/PolicyView";
import type { SettingsView } from "../bindings/SettingsView";
import type { StartedTask } from "../bindings/StartedTask";
import type { TaskSummary } from "../bindings/TaskSummary";
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

  listPendingApprovals: () => call<ApprovalView[]>("list_pending_approvals"),
  resolveApproval: (input: ApprovalDecisionInput) => call<boolean>("resolve_approval", { input }),

  activity: (limit?: number) => call<EventView[]>("activity", { limit: limit ?? null }),
  verifyAudit: () => call<string[]>("verify_audit"),

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
