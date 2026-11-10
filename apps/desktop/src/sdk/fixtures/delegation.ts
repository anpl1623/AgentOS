/** A supervisor's delegation tree: its runs, traces, approval and tasks. */

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { DelegationTreeView } from "../../bindings/DelegationTreeView";
import type { DelegationView } from "../../bindings/DelegationView";
import type { ExecutionView } from "../../bindings/ExecutionView";
import type { RunSummary } from "../../bindings/RunSummary";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { TraceView } from "../../bindings/TraceView";

import { agents } from "./agents";
import { minutesAgo } from "./clock";

/**
 * A delegation tree two levels deep, still working.
 *
 * The supervisor first asked the writer to draft a briefing; the writer ran
 * out of steps, and its failure came back as a result the supervisor read.
 * It then asked the researcher, whose call is still in progress and so has
 * no record yet. The researcher read a page from outside the trust boundary,
 * which taints the whole tree, since the tree shares one tracker, and asked
 * the reader to go further; the reader is now waiting on a person, and its
 * card names the chain that chose the work.
 */
export const delegationFixtures = (() => {
  const WEB = "web:https://globex.example/investors";
  const ids = {
    supervisor: { task: "task-0004", run: "run-0004" },
    writer: { task: "task-0006", run: "run-0006" },
    researcher: { task: "task-0005", run: "run-0005" },
    reader: { task: "task-0007", run: "run-0007" },
  } as const;
  const objectives = {
    supervisor: "Prepare a briefing on Globex before Thursday's renewal call.",
    writer: "Draft the Globex briefing from the CRM notes.",
    researcher: "Collect what Globex has announced this quarter.",
    reader: "Read the Globex newsroom and list this quarter's announcements.",
  } as const;
  type Name = keyof typeof ids;

  const runOf = (name: Name, extra: Partial<RunSummary>): RunSummary => ({
    id: ids[name].run,
    attempt: 1,
    state: "executing",
    tainted: true,
    taint_sources: [WEB],
    steps: 0,
    result: null,
    failure: null,
    input_tokens: 0,
    output_tokens: 0,
    started_at: minutesAgo(30),
    completed_at: null,
    ...extra,
  });
  const runs: Record<Name, RunSummary> = {
    supervisor: runOf("supervisor", { steps: 3, input_tokens: 3_120, output_tokens: 410 }),
    // Failed before anything in the tree read the page, so it is clean.
    writer: runOf("writer", {
      state: "failed",
      tainted: false,
      taint_sources: [],
      steps: 8,
      failure: "step budget of 24 exhausted",
      input_tokens: 9_870,
      output_tokens: 1_502,
      started_at: minutesAgo(29),
      completed_at: minutesAgo(26),
    }),
    researcher: runOf("researcher", {
      steps: 4,
      input_tokens: 4_410,
      output_tokens: 690,
      started_at: minutesAgo(24),
    }),
    reader: runOf("reader", {
      state: "waiting_for_approval",
      steps: 2,
      input_tokens: 1_980,
      output_tokens: 240,
      started_at: minutesAgo(20),
    }),
  };

  const delegation = (
    id: string,
    parent: Name,
    child: Name,
    depth: number,
    state: string,
    requested: number,
    ended: number | null,
  ): DelegationView => ({
    id,
    parent_run_id: ids[parent].run,
    parent_agent: parent,
    child_agent: child,
    objective: objectives[child],
    state,
    child_run_id: ids[child].run,
    steps: runs[child].steps,
    tainted: runs[child].tainted,
    depth,
    requested_at: minutesAgo(requested),
    duration_ms: ended === null ? null : (requested - ended) * 60_000,
  });
  const toWriter = delegation("dlg-0001", "supervisor", "writer", 1, "failed", 29, 26);
  const toResearcher = delegation("dlg-0002", "supervisor", "researcher", 1, "running", 24, null);
  const toReader = delegation("dlg-0003", "researcher", "reader", 2, "running", 20, null);

  const step = (ordinal: number, kind: string, minute: number, summary: string, call?: string) => ({
    ordinal,
    kind,
    state: kind === "tool_call" ? "executing" : "planning",
    summary,
    tool_execution_id: call ?? null,
    at: minutesAgo(minute),
  });
  const call = (
    id: string,
    run: Name,
    tool: string,
    args: unknown,
    minute: number,
    durationMs: number,
    risk: string,
    tainted: boolean,
  ): ExecutionView => ({
    id,
    run_id: ids[run].run,
    tool,
    call_id: `c-${id}`,
    arguments: JSON.stringify(args, null, 2),
    outcome: "success",
    executed: true,
    effect: "allow",
    risk,
    tainted,
    approval_id: null,
    duration_ms: durationMs,
    error: null,
    started_at: minutesAgo(minute),
  });

  const approval: ApprovalView = {
    id: "aa11bb22-3333-4444-5555-00000000d001",
    agent_name: "reader",
    delegation_chain: ["supervisor", "researcher", "reader"],
    task_id: ids.reader.task,
    run_id: ids.reader.run,
    objective: objectives.reader,
    tool: "network.request",
    arguments: JSON.stringify({ method: "GET", url: "https://globex.example/newsroom" }, null, 2),
    risk: "medium",
    reason: "rule `network.fetch` matched; escalated because this run has read untrusted data",
    effect_before_taint: "allow",
    asked_this_run: 1,
    approval_budget: 10,
    explanation: "GET https://globex.example/newsroom and return the response.",
    affected_resources: ["origin:https://globex.example"],
    tainted: true,
    taint_sources: [WEB],
    status: "pending",
    requested_at: minutesAgo(2),
    decided_at: null,
    note: null,
  };

  const base = (name: Name) => ({
    run: runs[name],
    task_id: ids[name].task,
    agent_name: name,
    objective: objectives[name],
    approvals: [] as ApprovalView[],
    delegations: [] as DelegationView[],
    delegated_by: null as DelegationView | null,
  });
  const traces: Record<string, TraceView> = {
    [ids.supervisor.run]: {
      ...base("supervisor"),
      steps: [
        step(1, "planning", 30, "The writer can draft from the CRM notes; asking it first."),
        step(2, "tool_call", 26, `Ask writer to: ${objectives.writer} → success`, "exec-s0"),
        step(3, "planning", 25, "The writer ran out of steps. Asking the researcher for sources."),
      ],
      executions: [
        call(
          "exec-s0",
          "supervisor",
          "agent.delegate",
          { requests: [{ agent: "writer", objective: objectives.writer }] },
          29,
          180_000,
          "high",
          false,
        ),
      ],
      delegations: [toWriter, toResearcher],
    },
    [ids.writer.run]: {
      ...base("writer"),
      steps: [
        step(1, "planning", 29, "Reading the CRM notes for Globex."),
        step(2, "recovery", 26, "step budget of 24 exhausted"),
      ],
      executions: [],
      delegated_by: toWriter,
    },
    [ids.researcher.run]: {
      ...base("researcher"),
      steps: [
        step(1, "planning", 24, "Starting from the investor page."),
        step(2, "tool_call", 23, "GET https://globex.example/investors → success", "exec-r0"),
        step(3, "planning", 21, "The newsroom has the detail; asking the reader to list it."),
      ],
      executions: [
        call(
          "exec-r0",
          "researcher",
          "network.request",
          { method: "GET", url: "https://globex.example/investors" },
          23,
          412,
          "medium",
          false,
        ),
      ],
      delegations: [toReader],
      delegated_by: toResearcher,
    },
    [ids.reader.run]: {
      ...base("reader"),
      steps: [step(1, "planning", 20, "Opening the newsroom.")],
      executions: [],
      approvals: [approval],
      delegated_by: toReader,
    },
  };

  const node = (name: Name, children: DelegationTreeView[] = []): DelegationTreeView => ({
    run_id: ids[name].run,
    agent: name,
    objective: objectives[name],
    state: runs[name].state,
    tainted: runs[name].tainted,
    steps: runs[name].steps,
    children,
  });
  const readerNode = node("reader");
  const researcherNode = node("researcher", [readerNode]);
  const trees: Record<string, DelegationTreeView> = {
    [ids.supervisor.run]: node("supervisor", [node("writer"), researcherNode]),
    [ids.researcher.run]: researcherNode,
    [ids.writer.run]: node("writer"),
    [ids.reader.run]: readerNode,
  };

  const task = (name: Name, status: string): TaskSummary => ({
    id: ids[name].task,
    objective: objectives[name],
    status,
    agent_name: name,
    agent_id: agents.find((agent) => agent.name === name)!.id,
    created_at: runs[name].started_at,
    completed_at: runs[name].completed_at,
    latest_run: runs[name],
  });

  return {
    traces,
    trees,
    approval,
    tasks: [task("supervisor", "running")],
    runsByTask: Object.fromEntries(
      (Object.keys(ids) as Name[]).map((name) => [ids[name].task, [runs[name]]]),
    ) as Record<string, RunSummary[]>,
  };
})();
