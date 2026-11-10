/** Which fixture answers which command, and the invoke that looks it up. */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { AuditVerificationView } from "../../bindings/AuditVerificationView";
import type { CadenceInput } from "../../bindings/CadenceInput";
import type { MemoryView } from "../../bindings/MemoryView";
import type { PolicyView } from "../../bindings/PolicyView";
import type { ScheduleView } from "../../bindings/ScheduleView";
import type { SettingsView } from "../../bindings/SettingsView";
import type { StartedTask } from "../../bindings/StartedTask";
import type { TaskNodeView } from "../../bindings/TaskNodeView";

import { activity, auditHealth, auditRecords } from "./activity";
import { MEMORY_KINDS, agents, checkPolicy, grants, memories, unknownKind } from "./agents";
import { pendingApproval, recentApprovals } from "./approvals";
import { delegationFixtures } from "./delegation";
import {
  bindIntegration,
  credentials,
  integrations,
  removeNetworkCredential,
  setNetworkCredential,
  testIntegration,
  unbindIntegration,
} from "./integrations";
import { Refusal } from "./refusal";
import { checkCadence, graph, scheduler, schedules, setSchedulerRunning } from "./schedules";
import { knownProvider, providers, removeProviderKey, settings } from "./settings";
import { dashboard, retryTask, runsByTask, tasks, trace } from "./tasks";
import { toolUsageOver } from "./usage";

const answers: Record<string, unknown> = {
  dashboard,
  list_agents: agents,
  list_tasks: tasks,
  list_pending_approvals: [pendingApproval, delegationFixtures.approval],
  list_recent_approvals: recentApprovals,
  // One gap, so the full check's second list can be seen: the run the last
  // process abandoned left a call it had started with no record of an end.
  verify_audit: {
    breaks: [],
    gaps: [
      "run run-0003: `filesystem.list` started at record 388 (execution exec-9) and nothing " +
        "records it finishing",
    ],
  } satisfies AuditVerificationView,
  audit_health: auditHealth,
  list_tools: settings.tools,
  get_task_trace: trace,
  resolve_approval: true,
  cancel_run: true,
  scheduler_status: scheduler,
  list_schedules: schedules,
  task_graph: graph,
  grant_report: grants,
  add_task_dependency: null,
  delete_schedule: null,
  forget_memory: null,
  acknowledge_close: null,
  confirm_close: null,
  get_agent: {
    summary: agents[0],
    instructions: "You handle sales follow-ups. Never send anything without approval.",
    policy: {
      document:
        "default: deny\nmax_risk: high\n\ntaint_escalation:\n  enabled: true\n  escalate_at_or_above: high\n\npermissions:\n  browser:\n    navigate: ['http://127.0.0.1:8420']\n    read: ['http://127.0.0.1:8420']\n",
      version: 3,
      default_effect: "deny",
      max_risk: "high",
      taint_enabled: true,
      taint_threshold: "high",
      rules: [
        "browser.navigate => allow on [origin:http://127.0.0.1:8420]",
        "browser.read => allow on [origin:http://127.0.0.1:8420]",
      ],
    },
    recent_tasks: tasks,
    workspace: "/Users/you/.agentos/workspace/sales",
  },
};

/** Answers that depend on the command's arguments. */
const computed: Record<string, (args: Record<string, unknown>) => unknown> = {
  tool_usage: (args) => toolUsageOver(args.days),
  settings: () => ({ ...settings, providers }) satisfies SettingsView,
  list_network_credentials: () => credentials,
  set_network_credential: setNetworkCredential,
  remove_network_credential: removeNetworkCredential,
  list_integrations: () => integrations,
  bind_integration: bindIntegration,
  unbind_integration: unbindIntegration,
  test_integration: testIntegration,
  set_provider_key: (args) => {
    knownProvider(String(args.provider));
    if (String(args.key ?? "").trim() === "") throw new Refusal("no key was provided");
    return null;
  },
  remove_provider_key: removeProviderKey,
  check_policy: (args) => checkPolicy(String(args.document ?? "")),
  set_policy: (args) => {
    const check = checkPolicy(String(args.document ?? ""));
    if (!check.valid || check.summary === null) {
      throw new Refusal(`this policy does not compile: ${check.error ?? "unknown error"}`);
    }
    return { ...check.summary, version: 4 } satisfies PolicyView;
  },
  start_task: (args) => {
    if (String(args.objective ?? "").trim() === "") {
      throw new Refusal("an objective is required");
    }
    const stamp = Date.now();
    return { task_id: `task-${stamp}`, run_id: `run-${stamp}` } satisfies StartedTask;
  },
  create_agent: (args) => {
    const input = args.input as {
      name: string;
      provider: string;
      model: string;
      tools: string[];
    };
    const unknown = input.tools.find(
      (tool) => !settings.tools.some((candidate) => candidate.name === tool),
    );
    if (unknown !== undefined) throw new Refusal(`unknown tool \`${unknown}\``);
    return {
      id: `agent-${input.name}`,
      name: input.name,
      provider: input.provider,
      model: input.model,
      status: "enabled",
      tools: input.tools,
      max_steps: 24,
      created_at: new Date().toISOString(),
    } satisfies AgentSummary;
  },
  set_agent_enabled: (args) => {
    const agent = agents.find((candidate) => candidate.name === args.name);
    if (!agent) throw new Refusal("there is no agent with that name");
    return { ...agent, status: args.enabled === true ? "enabled" : "disabled" };
  },
  activity: (args) =>
    args.securityOnly === true ? activity.filter((event) => event.security_relevant) : activity,
  audit_record: (args) => {
    const record = auditRecords.find((candidate) => candidate.id === args.eventId);
    if (!record) throw new Refusal("there is no audit record with that identity");
    return record;
  },
  audit_for_run: (args) => auditRecords.filter((record) => record.run_id === args.runId),
  list_runs: (args) => runsByTask[String(args.taskId)] ?? [],
  // Any run the fixtures do not describe answers with the CRM run, as every
  // run did before there was more than one trace to show.
  get_trace: (args) => delegationFixtures.traces[String(args.runId)] ?? trace,
  delegation_tree: (args) => {
    const tree = delegationFixtures.trees[String(args.runId)];
    if (!tree) throw new Refusal("there is no run with that identity");
    return tree;
  },
  retry_task: (args) => retryTask(String(args.taskId)),
  set_scheduler_running: setSchedulerRunning,
  check_cadence: (args) => checkCadence(args.cadence as CadenceInput),
  set_schedule_paused: (args) => {
    const schedule = schedules.find((candidate) => candidate.id === args.scheduleId);
    if (!schedule) throw new Refusal("there is no schedule with that identity");
    return { ...schedule, status: args.paused === true ? "paused" : "active" };
  },
  create_schedule: (args) => {
    const input = args.input as {
      agent_id: string;
      name: string;
      objective: string;
      cadence: CadenceInput;
      first_run_at: string | null;
    };
    const preview = checkCadence(input.cadence);
    if (!preview.valid) throw new Refusal(preview.error ?? "invalid cadence");
    if (input.cadence.kind === "once" && input.first_run_at === null) {
      throw new Refusal("a schedule that fires once needs a time to say when");
    }
    return {
      ...schedules[0]!,
      id: `schedule-${input.name}`,
      name: input.name,
      objective: input.objective,
      agent_id: input.agent_id,
      cadence: {
        kind: input.cadence.kind,
        seconds: input.cadence.seconds,
        expression: input.cadence.expression,
        clock: input.cadence.kind === "cron" ? (input.cadence.clock ?? "utc") : null,
        description: preview.description ?? "",
      },
      next_run_at: input.first_run_at ?? preview.next_runs[0] ?? null,
      last_run_at: null,
      last_task_id: null,
      created_at: new Date().toISOString(),
    } satisfies ScheduleView;
  },
  list_memories: (args) => {
    const kind = args.kind as string | null | undefined;
    if (kind != null && !MEMORY_KINDS.includes(kind)) {
      throw unknownKind(kind);
    }
    return memories.filter(
      (memory) => memory.agent_id === args.agentId && (kind == null || memory.kind === kind),
    );
  },
  remember: (args) => {
    const input = args.input as {
      agent_id: string;
      kind: string;
      content: string;
      confidence: number | null;
    };
    if (!MEMORY_KINDS.includes(input.kind)) {
      throw unknownKind(input.kind);
    }
    // Whatever the caller sends, the runtime records a remembered note as
    // typed by a person, so the fixture does too.
    return {
      ...memories[0]!,
      id: `memory-${Date.now()}`,
      agent_id: input.agent_id,
      kind: input.kind,
      content: input.content,
      source: "user",
      source_untrusted: false,
      reaches_the_prompt: ["fact", "decision", "preference"].includes(input.kind),
      confidence: input.confidence ?? 1,
      task_id: null,
    } satisfies MemoryView;
  },
  revise_memory: (args) => {
    const memory = memories.find((candidate) => candidate.id === args.memoryId);
    if (!memory) throw new Refusal("there is no memory with that identity");
    return {
      ...memory,
      content: String(args.content),
      confidence: typeof args.confidence === "number" ? args.confidence : memory.confidence,
      updated_at: new Date().toISOString(),
    };
  },
  create_task: (args) => {
    const input = args.input as {
      agent_id: string;
      objective: string;
      depends_on: string[];
      scheduled_for: string | null;
    };
    const missing = input.depends_on.find((id) => !graph.some((node) => node.id === id));
    if (missing) throw new Refusal(`task ${missing} does not exist, so nothing can wait for it`);
    return {
      ...graph[0]!,
      id: `task-${Date.now()}`,
      objective: input.objective,
      agent_id: input.agent_id,
      status: input.depends_on.length > 0 ? "blocked" : "pending",
      scheduled_for: input.scheduled_for,
      created_at: new Date().toISOString(),
      blocked_by: input.depends_on,
      blocks: [],
    } satisfies TaskNodeView;
  },
};

/** Answer a command with fixture data. */
export function fixtureInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const compute = computed[command];
  if (!compute && !(command in answers)) {
    return Promise.reject(
      new Error(`No fixture for \`${command}\`. Run inside the desktop window for real data.`),
    );
  }
  // A short delay so loading states are visible while building them.
  return new Promise((resolve, reject) => {
    setTimeout(() => {
      try {
        resolve((compute ? compute(args ?? {}) : answers[command]) as T);
      } catch (error) {
        reject(error instanceof Refusal ? error.message : error);
      }
    }, 120);
  });
}
