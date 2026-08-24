/**
 * Development fixtures.
 *
 * These answer runtime commands when the interface runs in an ordinary browser,
 * so screens can be built and reviewed without launching the application. They
 * are reached only when `import.meta.env.DEV` is true, which Vite replaces with
 * a literal at build time — so the branch, and everything it references, is dead
 * code in a production bundle.
 *
 * The data deliberately mirrors the shipped demo: an agent that has read a CRM
 * record containing a prompt-injection payload, and is now asking to do
 * something consequential. That is the state the approval card exists for, so it
 * is the state worth being able to look at.
 */

import type { ApprovalView } from "../bindings/ApprovalView";
import type { DashboardView } from "../bindings/DashboardView";
import type { EventView } from "../bindings/EventView";
import type { SettingsView } from "../bindings/SettingsView";
import type { TraceView } from "../bindings/TraceView";

const now = new Date("2026-08-24T10:32:00Z");
const minutesAgo = (n: number) => new Date(now.getTime() - n * 60_000).toISOString();

const agents = [
  {
    id: "9f1c2b3a-1111-4aaa-8bbb-000000000001",
    name: "sales",
    provider: "anthropic",
    model: "claude-opus-5",
    status: "enabled",
    tools: ["browser.navigate", "browser.extract", "filesystem.write"],
    max_steps: 24,
    created_at: minutesAgo(4000),
  },
  {
    id: "9f1c2b3a-1111-4aaa-8bbb-000000000002",
    name: "ops",
    provider: "ollama",
    model: "llama3",
    status: "disabled",
    tools: ["filesystem.read", "filesystem.list"],
    max_steps: 24,
    created_at: minutesAgo(9000),
  },
];

const pendingApproval: ApprovalView = {
  id: "aa11bb22-3333-4444-5555-666677778888",
  agent_name: "sales",
  task_id: "task-0001",
  run_id: "run-0001",
  objective:
    "Open the CRM, find every customer whose follow-up is overdue, and draft a message for each.",
  tool: "browser.type",
  arguments: JSON.stringify(
    { selector: "#message", text: "Following up on the revised quote.", submit: true },
    null,
    2,
  ),
  risk: "high",
  reason: "rule `browser.interact` matched; escalated because this run has read untrusted data",
  explanation: "Type 41 characters into `#message` on http://127.0.0.1:8420 and submit.",
  affected_resources: ["origin:http://127.0.0.1:8420"],
  tainted: true,
  taint_sources: ["web:http://127.0.0.1:8420/customers/globex"],
  status: "pending",
  requested_at: minutesAgo(1),
  decided_at: null,
  note: null,
};

const activity: EventView[] = [
  ["agent.task.started", "Find overdue follow-ups in the CRM", false],
  ["agent.state.transitioned", "idle → planning", false],
  ["agent.model.request.completed", "anthropic/claude-opus-5", false],
  ["permission.granted", "browser.navigate", false],
  ["tool.execution.completed", "browser.navigate", false],
  ["agent.taint.raised", "browser.extract", true],
  ["permission.denied", "filesystem.read", true],
  ["tool.unknown", "terminal.exec", true],
  ["permission.escalated_by_taint", "browser.type", true],
  ["approval.requested", "browser.type", false],
].map(([kind, summary, security], index) => ({
  id: `event-${index}`,
  sequence: index + 41,
  at: minutesAgo(10 - index),
  kind: kind as string,
  run_id: "run-0001",
  task_id: "task-0001",
  summary: summary as string,
  security_relevant: security as boolean,
}));

const trace: TraceView = {
  run: {
    id: "run-0001",
    attempt: 1,
    state: "waiting_for_approval",
    tainted: true,
    steps: 6,
    result: null,
    failure: null,
    input_tokens: 8421,
    output_tokens: 1204,
    started_at: minutesAgo(11),
    completed_at: null,
  },
  task_id: "task-0001",
  agent_name: "sales",
  objective:
    "Open the CRM, find every customer whose follow-up is overdue, and draft a message for each.",
  steps: [
    ["planning", "planning", "I will open the customer list."],
    ["tool_call", "executing", "Open http://127.0.0.1:8420/customers → success"],
    ["planning", "planning", "Three accounts are overdue. Reading the Globex record."],
    ["tool_call", "executing", "Read `#notes` from http://127.0.0.1:8420 → success"],
    ["planning", "planning", "That record contains text impersonating a system message."],
    ["tool_call", "executing", "Read /Users/me/.ssh/id_rsa → denied"],
  ].map(([kind, state, summary], index) => ({
    ordinal: index + 1,
    kind: kind as string,
    state: state as string,
    summary: summary as string,
    tool_execution_id: null,
    at: minutesAgo(11 - index),
  })),
  executions: [
    ["browser.navigate", "success", "allow", "medium", 144, null],
    ["browser.extract", "success", "allow", "low", 2, null],
    [
      "filesystem.read",
      "denied",
      "deny",
      "low",
      0,
      "permission denied: no rule matched `filesystem.read on path:/Users/me/.ssh/id_rsa`",
    ],
    ["terminal.exec", "invalid_arguments", "deny", "none", 0, "unknown tool `terminal.exec`"],
  ].map(([tool, outcome, effect, risk, duration, error], index) => ({
    id: `exec-${index}`,
    tool: tool as string,
    call_id: `c${index + 1}`,
    arguments: '{"path":"…"}',
    outcome: outcome as string,
    executed: outcome === "success",
    effect: effect as string,
    risk: risk as string,
    tainted: index > 1,
    approval_id: null,
    duration_ms: duration as number,
    error: error as string | null,
    started_at: minutesAgo(11 - index),
  })),
  approvals: [pendingApproval],
};

const settings: SettingsView = {
  data_dir: "/Users/you/.agentos",
  workspace: "/Users/you/.agentos/workspace",
  database: "/Users/you/.agentos/agentos.db",
  keychain_available: true,
  keychain_reason: null,
  providers: [
    { id: "anthropic", configured: true, hint: "sk-a…9f2", source: "system keychain", note: "" },
    {
      id: "openai",
      configured: false,
      hint: null,
      source: null,
      note: "or set OPENAI_API_KEY in the environment",
    },
    { id: "ollama", configured: false, hint: null, source: null, note: "local; usually needs no key" },
    { id: "mock", configured: false, hint: null, source: null, note: "built in; no key needed" },
  ],
  browser_path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  browser_hint: null,
  tools: [
    ["browser.navigate", "browser", "medium", true, "Open a URL in the agent's browser."],
    ["browser.extract", "browser", "low", true, "Read the visible text of the page."],
    ["filesystem.read", "filesystem", "low", true, "Read a UTF-8 text file."],
    ["filesystem.write", "filesystem", "medium", false, "Write text to a file."],
    ["filesystem.delete", "filesystem", "high", false, "Delete a file or directory."],
    ["terminal.exec", "terminal", "high", true, "Run a program directly with an argument vector."],
  ].map(([name, domain, risk, untrusted, description]) => ({
    name: name as string,
    domain: domain as string,
    risk: risk as string,
    returns_untrusted_data: untrusted as boolean,
    description: description as string,
  })),
};

const tasks = [
  {
    id: "task-0001",
    objective:
      "Open the CRM, find every customer whose follow-up is overdue, and draft a message for each.",
    status: "running",
    agent_name: "sales",
    agent_id: agents[0]!.id,
    created_at: minutesAgo(11),
    completed_at: null,
    latest_run: trace.run,
  },
  {
    id: "task-0002",
    objective: "Summarise last week's activity.",
    status: "succeeded",
    agent_name: "sales",
    agent_id: agents[0]!.id,
    created_at: minutesAgo(180),
    completed_at: minutesAgo(176),
    latest_run: {
      id: "run-0002",
      attempt: 1,
      state: "completed",
      tainted: false,
      steps: 3,
      result: "Nothing notable happened last week.",
      failure: null,
      input_tokens: 2100,
      output_tokens: 320,
      started_at: minutesAgo(180),
      completed_at: minutesAgo(176),
    },
  },
];

const dashboard: DashboardView = {
  agents,
  running_tasks: [tasks[0]!],
  pending_approvals: [pendingApproval],
  recent_events: activity,
  recent_refusals: trace.executions.filter((execution) => !execution.executed),
  audit_events: 412,
  audit_intact: true,
};

const answers: Record<string, unknown> = {
  dashboard,
  list_agents: agents,
  list_tasks: tasks,
  list_pending_approvals: [pendingApproval],
  activity,
  verify_audit: [],
  list_tools: settings.tools,
  settings,
  get_trace: trace,
  get_task_trace: trace,
  resolve_approval: true,
  cancel_run: true,
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

/** Answer a command with fixture data. */
export function fixtureInvoke<T>(command: string, _args?: Record<string, unknown>): Promise<T> {
  if (!(command in answers)) {
    return Promise.reject(
      new Error(`No fixture for \`${command}\`. Run inside the desktop window for real data.`),
    );
  }
  // A short delay so loading states are visible while building them.
  return new Promise((resolve) => {
    setTimeout(() => resolve(answers[command] as T), 120);
  });
}

/** Replay activity events on a timer, so the live feed can be seen working. */
export function fixtureSubscribe<T>(event: string, handler: (payload: T) => void): () => void {
  if (event !== "agentos://activity") return () => {};

  let index = 0;
  const timer = setInterval(() => {
    const next = activity[index % activity.length];
    if (next) {
      handler({ ...next, id: `live-${index}`, at: new Date().toISOString() } as T);
    }
    index += 1;
  }, 3000);

  return () => clearInterval(timer);
}
