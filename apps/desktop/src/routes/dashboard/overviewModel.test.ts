import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { RunSummary } from "../../bindings/RunSummary";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { NOT_YET, type Tile } from "./dashboardModel";
import {
  LONG_WAIT_MS,
  agentActivity,
  auditSummary,
  callsTotal,
  decisionSummary,
  greeting,
  longestWait,
  runClock,
  runMotion,
  runningBreakdown,
  runningHint,
  statusLine,
  stepBudget,
  toolsCalled,
  waitedLong,
} from "./overviewModel";

const NOW = Date.parse("2026-10-06T12:00:00Z");
const secondsAgo = (n: number) => new Date(NOW - n * 1000).toISOString();
const minutesAgo = (n: number) => secondsAgo(n * 60);

function approval(id: string, waited: number, extra: Partial<ApprovalView> = {}): ApprovalView {
  return {
    id,
    agent_name: "sales",
    delegation_chain: [],
    task_id: "task-1",
    run_id: "run-1",
    objective: "Follow up with every overdue customer.\nThen report.",
    tool: "browser.type",
    arguments: "{}",
    risk: "high",
    reason: "rule asks",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: 10,
    explanation: "Type 41 characters into `#message`.",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: minutesAgo(waited),
    decided_at: null,
    note: null,
    ...extra,
  };
}

function run(state: string): RunSummary {
  return {
    id: `run-${state}`,
    attempt: 1,
    state,
    tainted: false,
    taint_sources: [],
    steps: 3,
    result: null,
    failure: null,
    input_tokens: 0,
    output_tokens: 0,
    started_at: minutesAgo(5),
    completed_at: null,
  };
}

function task(id: string, latest: RunSummary | null, agentId = "agent-sales"): TaskSummary {
  return {
    id,
    objective: `Objective ${id}`,
    status: "running",
    agent_name: "sales",
    agent_id: agentId,
    created_at: minutesAgo(10),
    completed_at: null,
    latest_run: latest,
  };
}

function usage(tool: string, calls: number): ToolUsageView {
  return {
    tool,
    calls,
    executed: calls,
    denied: 0,
    approval_denied: 0,
    failed: 0,
    under_taint: 0,
    approvals: 0,
    total_duration_ms: 0,
    last_used_at: null,
    risk: "low",
    returns_untrusted_data: false,
    daily: [calls],
  };
}

const intact: Tile = { value: "intact", label: "audit chain", tone: "ok" };

describe("greeting", () => {
  it("greets by the part of the day", () => {
    expect(greeting(8)).toBe("Good morning");
    expect(greeting(12)).toBe("Good afternoon");
    expect(greeting(17)).toBe("Good afternoon");
    expect(greeting(18)).toBe("Good evening");
    expect(greeting(23)).toBe("Good evening");
  });

  it("does not call the small hours morning", () => {
    expect(greeting(0)).toBe("Working late");
    expect(greeting(4)).toBe("Working late");
    expect(greeting(5)).toBe("Good morning");
  });

  it("wraps an hour outside the clock and says hello to one it cannot read", () => {
    expect(greeting(26)).toBe("Working late");
    expect(greeting(-1)).toBe("Good evening");
    expect(greeting(9.75)).toBe("Good morning");
    expect(greeting(Number.NaN)).toBe("Hello");
  });
});

describe("statusLine", () => {
  const base = { fixtures: false, feedFailing: false, chain: intact, missing: 0 };

  it("says what the window is connected to, as the sidebar does", () => {
    expect(statusLine(base)[0]).toMatchObject({ id: "runtime", tone: "ok", mark: "live" });
    expect(statusLine({ ...base, feedFailing: true })[0]).toMatchObject({
      tone: "warn",
      text: "Runtime not answering",
    });
    expect(statusLine({ ...base, fixtures: true, feedFailing: true })[0]).toMatchObject({
      tone: "warn",
      text: "Fixture data — not connected to a runtime",
    });
  });

  it("follows the chain tile, keeping a break or a gap in the danger hue", () => {
    const chain = (value: string) =>
      statusLine({ ...base, chain: { value, label: "audit chain" } }).find(
        (part) => part.id === "chain",
      );
    expect(chain("intact")).toMatchObject({
      tone: "ok",
      mark: "shield",
      text: "Audit chain verified",
    });
    expect(chain("broken")).toMatchObject({ tone: "danger", text: "Audit chain broken" });
    expect(chain("incomplete")).toMatchObject({ tone: "danger" });
    expect(chain("unknown")).toMatchObject({ tone: "warn", text: "Audit chain not confirmed" });
    expect(chain(NOT_YET)).toMatchObject({ tone: "neutral" });
  });

  it("says nothing about setup until it has been read, then counts what is left", () => {
    expect(statusLine({ ...base, missing: null }).map((part) => part.id)).toEqual([
      "runtime",
      "chain",
    ]);
    expect(statusLine(base)[2]).toMatchObject({ tone: "ok", text: "Setup complete" });
    expect(statusLine({ ...base, missing: 1 })[2]?.text).toBe("1 setup step left");
    expect(statusLine({ ...base, missing: 3 })[2]).toMatchObject({
      tone: "warn",
      text: "3 setup steps left",
    });
  });
});

describe("auditSummary", () => {
  it("capitalises the tile's word and counts the records", () => {
    expect(auditSummary(intact, 412)).toEqual({ state: "Intact", detail: "412 records" });
    expect(auditSummary(intact, 12_400).detail).toBe("12.4k records");
  });

  it("keeps what the tile's label added, such as records that went unwritten", () => {
    const tile: Tile = { value: "incomplete", label: "audit chain · 3 unwritten", tone: "danger" };
    expect(auditSummary(tile, 40)).toEqual({
      state: "Incomplete",
      detail: "40 records · 3 unwritten",
    });
  });

  it("says the records are not yet counted before the first answer", () => {
    expect(auditSummary({ value: NOT_YET, label: "audit chain" }, null)).toEqual({
      state: NOT_YET,
      detail: "Not yet counted",
    });
  });
});

describe("tool call figures", () => {
  it("adds every call and counts the tools that had any", () => {
    const report = [usage("a", 3), usage("b", 0), usage("c", 9)];
    expect(callsTotal(report)).toBe(12);
    expect(toolsCalled(report)).toBe(2);
    expect(callsTotal([])).toBe(0);
  });

  // How today compares with the days before it is in usageModel.test.ts:
  // since the report counts whole local days, the comparison needs the
  // window's day-by-day series rather than its total.
});

describe("running", () => {
  it("splits the running list into working, waiting and queued", () => {
    const breakdown = runningBreakdown([
      task("a", run("executing")),
      task("b", run("waiting_for_approval")),
      task("c", run("planning")),
      task("d", null),
    ]);
    expect(breakdown).toEqual({ working: 2, waiting: 1, queued: 1 });
    expect(runningHint(breakdown)).toBe("2 working · 1 waiting on you · 1 queued");
    expect(runningHint({ working: 0, waiting: 0, queued: 0 })).toBeNull();
  });

  it("ticks to the second while seconds matter, and stops counting them after", () => {
    expect(runClock(secondsAgo(45), NOW)).toBe("45s");
    expect(runClock(secondsAgo(11 * 60 + 4), NOW)).toBe("11m 04s");
    expect(runClock(minutesAgo(125), NOW)).toBe("2h 05m");
    expect(runClock(minutesAgo(60 * 76), NOW)).toBe("3d 4h");
  });

  it("shows a dash for a time it cannot read, and zero for one in the future", () => {
    expect(runClock("not a time", NOW)).toBe("—");
    expect(runClock(secondsAgo(-30), NOW)).toBe("0s");
  });

  it("holds a run waiting on a person still, in the attention hue", () => {
    expect(runMotion("executing")).toEqual({ tone: "info", moving: true });
    expect(runMotion("planning")).toEqual({ tone: "info", moving: true });
    expect(runMotion("waiting_for_approval")).toEqual({ tone: "warn", moving: false });
    expect(runMotion("completed").moving).toBe(false);
  });

  it("measures a run against its agent's step limit, capped at the limit", () => {
    expect(stepBudget(6, 24)).toEqual({ percent: 25, label: "6 of 24 steps" });
    expect(stepBudget(30, 24)?.percent).toBe(100);
    expect(stepBudget(-1, 24)?.percent).toBe(0);
  });

  it("claims no budget when the limit is unknown", () => {
    expect(stepBudget(6, undefined)).toBeNull();
    expect(stepBudget(6, null)).toBeNull();
    expect(stepBudget(6, 0)).toBeNull();
  });

  it("knows which agents have work in the running list, and what that work is doing", () => {
    const activity = agentActivity([
      task("a", run("executing"), "agent-1"),
      task("b", null, "agent-2"),
      task("c", run("waiting_for_approval"), "agent-3"),
    ]);
    expect([...activity.keys()].sort()).toEqual(["agent-1", "agent-2", "agent-3"]);
    expect(activity.get("agent-1")).toBe("working");
    expect(activity.get("agent-2")).toBe("queued");
    expect(activity.get("agent-3")).toBe("waiting");
  });

  it("calls an agent working only while a run of its own is moving", () => {
    // The fixture's sales agent: its one run waits on a decision, so it must
    // not pulse beside a running card that holds still for the same reason.
    expect(agentActivity([task("a", run("waiting_for_approval"), "agent-1")]).get("agent-1")).toBe(
      "waiting",
    );
    expect(agentActivity([task("a", run("completed"), "agent-1")]).get("agent-1")).toBe("queued");
  });

  it("shows the liveliest of an agent's tasks, whatever order they come in", () => {
    const tasks = [
      task("a", null, "agent-1"),
      task("b", run("waiting_for_approval"), "agent-1"),
      task("c", run("executing"), "agent-1"),
    ];
    expect(agentActivity(tasks).get("agent-1")).toBe("working");
    expect(agentActivity([...tasks].reverse()).get("agent-1")).toBe("working");
    expect(agentActivity(tasks.slice(0, 2)).get("agent-1")).toBe("waiting");
  });
});

describe("needs you", () => {
  it("names how long the longest wait has been", () => {
    expect(longestWait([approval("a", 3), approval("b", 42), approval("c", 1)], NOW)).toBe("42m");
    expect(longestWait([], NOW)).toBeNull();
    expect(longestWait([approval("a", 0, { requested_at: "garbage" })], NOW)).toBeNull();
  });

  it("calls a wait out once it passes a quarter of an hour", () => {
    expect(waitedLong(minutesAgo(14), NOW)).toBe(false);
    expect(waitedLong(new Date(NOW - LONG_WAIT_MS).toISOString(), NOW)).toBe(true);
    expect(waitedLong("garbage", NOW)).toBe(false);
  });

  it("says what the agent wants to do, falling back to the task's objective", () => {
    expect(decisionSummary(approval("a", 1))).toBe("Type 41 characters into `#message`.");
    expect(decisionSummary(approval("a", 1, { explanation: "  " }))).toBe(
      "Follow up with every overdue customer. …",
    );
  });
});
