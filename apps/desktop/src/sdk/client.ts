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
import type { CadenceInput } from "../bindings/CadenceInput";
import type { CadencePreview } from "../bindings/CadencePreview";
import type { CreateAgentInput } from "../bindings/CreateAgentInput";
import type { CreateScheduleInput } from "../bindings/CreateScheduleInput";
import type { CreateTaskInput } from "../bindings/CreateTaskInput";
import type { DashboardView } from "../bindings/DashboardView";
import type { EventView } from "../bindings/EventView";
import type { IntegrationTestView } from "../bindings/IntegrationTestView";
import type { IntegrationView } from "../bindings/IntegrationView";
import type { MemoryView } from "../bindings/MemoryView";
import type { NetworkCredentialView } from "../bindings/NetworkCredentialView";
import type { PolicyCheck } from "../bindings/PolicyCheck";
import type { PolicyView } from "../bindings/PolicyView";
import type { RememberInput } from "../bindings/RememberInput";
import type { RunSummary } from "../bindings/RunSummary";
import type { ScheduleView } from "../bindings/ScheduleView";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { SettingsView } from "../bindings/SettingsView";
import type { StartedTask } from "../bindings/StartedTask";
import type { TaskNodeView } from "../bindings/TaskNodeView";
import type { TaskSummary } from "../bindings/TaskSummary";
import type { ToolGrantView } from "../bindings/ToolGrantView";
import type { ToolUsageView } from "../bindings/ToolUsageView";
import type { ToolView } from "../bindings/ToolView";
import type { TraceView } from "../bindings/TraceView";

import { call } from "./transport";

/** Events the runtime pushes to the window. */
export const events = {
  approvalRequested: "agentos://approval-requested",
  approvalResolved: "agentos://approval-resolved",
  activity: "agentos://activity",
  /**
   * The main window was asked to close while something was live. Carries a
   * `CloseGuard`; the window stays open until `confirmClose` is called.
   */
  closeRequested: "agentos://close-requested",
} as const;

export const api = {
  dashboard: () => call<DashboardView>("dashboard"),

  listAgents: () => call<AgentSummary[]>("list_agents"),
  getAgent: (name: string) => call<AgentDetail>("get_agent", { name }),
  createAgent: (input: CreateAgentInput) => call<AgentSummary>("create_agent", { input }),
  setAgentEnabled: (name: string, enabled: boolean) =>
    call<AgentSummary>("set_agent_enabled", { name, enabled }),

  /**
   * Per granted tool, how far its policy lets each capability it declares
   * reach: allowed everywhere, scoped to some resources, asking at best, or
   * denied. Computed by the permission engine, not by reading rule text.
   */
  grantReport: (agentId: string) => call<ToolGrantView[]>("grant_report", { agentId }),

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
  /**
   * Queue a task without running it: after other tasks succeed, at a moment,
   * or both. A queued task only starts on its own while the scheduler runs.
   */
  createTask: (input: CreateTaskInput) => call<TaskNodeView>("create_task", { input }),
  /** Make one queued task wait for another. A cycle is refused with its path. */
  addTaskDependency: (taskId: string, dependsOn: string) =>
    call<null>("add_task_dependency", { taskId, dependsOn }),
  /** Recent tasks as a graph, each with what it waits for and whether it can start. */
  taskGraph: (limit?: number) => call<TaskNodeView[]>("task_graph", { limit: limit ?? null }),

  /** Whether the scheduler runs in this window, and the work it would find. */
  schedulerStatus: () => call<SchedulerView>("scheduler_status"),
  /**
   * Turn the scheduler on or off, and remember the choice for the next launch.
   * Refused below a 5-second tick or with no concurrent runs.
   */
  setSchedulerRunning: (enabled: boolean, tickSeconds: number, maxConcurrentRuns: number) =>
    call<SchedulerView>("set_scheduler_running", { enabled, tickSeconds, maxConcurrentRuns }),

  listSchedules: () => call<ScheduleView[]>("list_schedules"),
  createSchedule: (input: CreateScheduleInput) =>
    call<ScheduleView>("create_schedule", { input }),
  /** Resuming computes the next occurrence forward from now; nothing missed is owed. */
  setSchedulePaused: (scheduleId: string, paused: boolean) =>
    call<ScheduleView>("set_schedule_paused", { scheduleId, paused }),
  /** The tasks the schedule already created are kept. */
  deleteSchedule: (scheduleId: string) => call<null>("delete_schedule", { scheduleId }),
  /** Check a cadence as it is typed, with its next few occurrences. */
  checkCadence: (cadence: CadenceInput) => call<CadencePreview>("check_cadence", { cadence }),

  /** An agent's memories, optionally of one kind. An unknown kind is refused. */
  listMemories: (agentId: string, kind?: string) =>
    call<MemoryView[]>("list_memories", { agentId, kind: kind ?? null }),
  /** Record a memory as written by a person; the source is not the caller's to set. */
  remember: (input: RememberInput) => call<MemoryView>("remember", { input }),
  reviseMemory: (memoryId: string, content: string, confidence?: number) =>
    call<MemoryView>("revise_memory", { memoryId, content, confidence: confidence ?? null }),
  forgetMemory: (memoryId: string) => call<null>("forget_memory", { memoryId }),

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

  /** Stored network credentials, by origin and name. No value ever crosses. */
  listNetworkCredentials: () => call<NetworkCredentialView[]>("list_network_credentials"),
  /**
   * Store a credential bound to one origin, replacing any of the same name
   * there. The answer names the origin as the runtime normalised it; the
   * secret is not echoed, and a refusal does not quote it.
   */
  setNetworkCredential: (origin: string, name: string, secret: string) =>
    call<NetworkCredentialView>("set_network_credential", { origin, name, secret }),
  removeNetworkCredential: (origin: string, name: string) =>
    call<null>("remove_network_credential", { origin, name }),

  /**
   * Every integration the runtime ships, with its tools and bound accounts.
   * An account says whether the keychain still holds its token; never the
   * token, nor a hint of it.
   */
  listIntegrations: () => call<IntegrationView[]>("list_integrations"),
  /**
   * Bind an account: its token is stored as a network credential for the
   * host's origin, then the account is recorded. `host` is the default when
   * null. Only this call can let an account reach a private network address.
   * Nothing is answered, so the token is not echoed, and a refusal does not
   * quote it; read the list again to see what was stored.
   */
  bindIntegration: (
    integration: string,
    label: string,
    host: string | null,
    privateNetwork: boolean,
    scopes: string | null,
    token: string,
  ) =>
    call<null>("bind_integration", { integration, label, host, privateNetwork, scopes, token }),
  /** Remove the account, then its token. */
  unbindIntegration: (accountId: string) => call<null>("unbind_integration", { accountId }),
  /** One authenticated read against the account's host, and what it found. */
  testIntegration: (accountId: string) =>
    call<IntegrationTestView>("test_integration", { accountId }),

  /**
   * Tell the runtime a held close has reached the interface. A close nobody
   * acknowledges is not held, so a window whose interface is gone still closes.
   */
  acknowledgeClose: () => call<null>("acknowledge_close"),
  /** Close the main window after the operator has confirmed what that stops. */
  confirmClose: () => call<null>("confirm_close"),
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
