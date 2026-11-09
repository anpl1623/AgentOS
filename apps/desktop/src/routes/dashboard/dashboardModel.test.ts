import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { AuditHealth } from "../../bindings/AuditHealth";
import type { ExecutionView } from "../../bindings/ExecutionView";
import type { RunSummary } from "../../bindings/RunSummary";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import {
  NEEDS_YOU_SHOWN,
  NEVER_RAN,
  NOT_YET,
  type ToolRow,
  chainTile,
  failureRows,
  firstLine,
  needsYou,
  refusalRows,
  refusedTotal,
  span,
  toolMeter,
  toolRows,
  unrecordedNote,
} from "./dashboardModel";
import { sortApprovals } from "../approvals/approvalsModel";

const NOW = Date.parse("2026-10-06T12:00:00Z");
const minutesAgo = (n: number) => new Date(NOW - n * 60_000).toISOString();

function approval(id: string, risk: string, waited: number): ApprovalView {
  return {
    id,
    agent_name: "sales",
    delegation_chain: [],
    task_id: "task-1",
    run_id: "run-1",
    objective: "Follow up",
    tool: "browser.type",
    arguments: "{}",
    risk,
    reason: "rule asks",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: 10,
    explanation: "",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: minutesAgo(waited),
    decided_at: null,
    note: null,
  };
}

function execution(id: string, at: number, extra: Partial<ExecutionView> = {}): ExecutionView {
  return {
    id,
    run_id: `run-${id}`,
    tool: "filesystem.read",
    call_id: "call-1",
    arguments: "{}",
    outcome: "denied",
    executed: false,
    effect: "deny",
    risk: "low",
    tainted: false,
    approval_id: null,
    duration_ms: 0,
    error: "no rule matched",
    started_at: minutesAgo(at),
    ...extra,
  };
}

function run(id: string, extra: Partial<RunSummary> = {}): RunSummary {
  return {
    id,
    attempt: 1,
    state: "failed",
    tainted: false,
    taint_sources: [],
    steps: 2,
    result: null,
    failure: "provider error",
    input_tokens: 0,
    output_tokens: 0,
    started_at: minutesAgo(30),
    completed_at: minutesAgo(20),
    ...extra,
  };
}

function task(
  id: string,
  latest: RunSummary | null,
  extra: Partial<TaskSummary> = {},
): TaskSummary {
  return {
    id,
    objective: `Objective ${id}`,
    status: "failed",
    agent_name: "ops",
    agent_id: "agent-ops",
    created_at: minutesAgo(60),
    completed_at: null,
    latest_run: latest,
    ...extra,
  };
}

function usage(tool: string, extra: Partial<ToolUsageView> = {}): ToolUsageView {
  return {
    tool,
    calls: 0,
    executed: 0,
    denied: 0,
    approval_denied: 0,
    failed: 0,
    under_taint: 0,
    approvals: 0,
    total_duration_ms: 0,
    last_used_at: null,
    risk: "low",
    returns_untrusted_data: false,
    ...extra,
  };
}

function row(extra: Partial<ToolRow>): ToolRow {
  return {
    tool: "t",
    risk: "low",
    calls: 0,
    executed: 0,
    failed: 0,
    refused: 0,
    totalMs: 0,
    lastUsedAt: null,
    mostlyRefused: false,
    ...extra,
  };
}

function health(extra: Partial<AuditHealth> = {}): AuditHealth {
  return { events: 412, intact: true, checked_at: minutesAgo(0), unrecorded: 0, ...extra };
}

describe("span", () => {
  it("says how long, in whole units rounded down, without an 'ago'", () => {
    expect(span(minutesAgo(0.5), NOW)).toBe("less than a minute");
    expect(span(minutesAgo(4.9), NOW)).toBe("4m");
    expect(span(minutesAgo(59), NOW)).toBe("59m");
    expect(span(minutesAgo(119), NOW)).toBe("1h");
    expect(span(minutesAgo(60 * 24 * 3 + 5), NOW)).toBe("3d");
  });

  it("says less than a minute for a time it cannot read or one in the future", () => {
    expect(span("not a time", NOW)).toBe("less than a minute");
    expect(span(minutesAgo(-10), NOW)).toBe("less than a minute");
  });
});

describe("firstLine", () => {
  it("keeps a single line whole and marks a cut one", () => {
    expect(firstLine("  denied  ")).toBe("denied");
    expect(firstLine("denied\nstack trace follows")).toBe("denied …");
  });
});

describe("needsYou", () => {
  it("puts the most dangerous first and the longest waiting first among equals", () => {
    const sorted = needsYou([
      approval("low-old", "low", 30),
      approval("high-new", "high", 1),
      approval("high-old", "high", 9),
      approval("critical", "critical", 2),
    ]).shown.map((each) => each.id);
    expect(sorted).toEqual(["critical", "high-old", "high-new", "low-old"]);
  });

  it("sorts a risk level it does not know above critical, where it will be seen", () => {
    const sorted = needsYou([
      approval("critical", "critical", 5),
      approval("new", "apocalyptic", 1),
    ]);
    expect(sorted.shown[0]?.id).toBe("new");
  });

  it("lists five and counts the rest, taking the five that matter most", () => {
    const queue = [
      ...Array.from({ length: 6 }, (_, index) => approval(`low-${index}`, "low", 60 - index)),
      approval("high", "high", 1),
    ];
    const result = needsYou(queue);
    expect(result.shown).toHaveLength(NEEDS_YOU_SHOWN);
    expect(result.more).toBe(2);
    expect(result.shown[0]?.id).toBe("high");
  });

  it("has nothing more when everything fits", () => {
    expect(needsYou([approval("a", "low", 1)]).more).toBe(0);
    expect(needsYou([])).toEqual({ shown: [], more: 0 });
  });

  it("lists the same five the approvals screen puts at the top, in its order", () => {
    const queue = [
      approval("medium", "medium", 3),
      approval("unreadable", "high", 0),
      approval("critical-new", "critical", 1),
      approval("low", "low", 40),
      approval("high", "high", 7),
      approval("critical-old", "critical", 8),
      approval("none", "none", 90),
    ];
    queue[1] = { ...queue[1]!, requested_at: "not a time" };
    expect(needsYou(queue).shown.map((each) => each.id)).toEqual(
      sortApprovals(queue)
        .slice(0, NEEDS_YOU_SHOWN)
        .map((each) => each.id),
    );
  });

  it("leaves the queue it was given in its own order", () => {
    const queue = [approval("low", "low", 1), approval("high", "high", 1)];
    needsYou(queue);
    expect(queue.map((each) => each.id)).toEqual(["low", "high"]);
  });
});

describe("refusalRows", () => {
  it("lists newest first, each opening the run that made the call", () => {
    const rows = refusalRows([execution("old", 30), execution("new", 2)]);
    expect(rows.map((row) => row.id)).toEqual(["new", "old"]);
    expect(rows[0]?.runId).toBe("run-new");
  });

  it("shows one line of the reason and keeps all of it for the title", () => {
    const [row] = refusalRows([
      execution("a", 1, { error: "no rule matched `filesystem.read`\nsecond line" }),
    ]);
    expect(row?.reason).toBe("no rule matched `filesystem.read` …");
    expect(row?.detail).toBe("no rule matched `filesystem.read`\nsecond line");
  });

  it("falls back to the outcome in words when there is no error text", () => {
    const [row] = refusalRows([execution("a", 1, { error: null, outcome: "approval_denied" })]);
    expect(row?.reason).toBe("approval denied");
  });

  it("carries the taint and the outcome through for the marker and the verdict", () => {
    const [row] = refusalRows([execution("a", 1, { tainted: true, outcome: "approval_denied" })]);
    expect(row?.tainted).toBe(true);
    expect(row?.outcome).toBe("approval_denied");
  });
});

describe("failureRows", () => {
  it("dates a failure by when its run ended, most recent first", () => {
    const rows = failureRows([
      task("older", run("run-older", { completed_at: minutesAgo(90) })),
      task("newer", run("run-newer", { completed_at: minutesAgo(5) })),
    ]);
    expect(rows.map((row) => row.id)).toEqual(["newer", "older"]);
    expect(rows[0]?.failedAt).toBe(minutesAgo(5));
    expect(rows[0]?.runId).toBe("run-newer");
  });

  it("gives a task that never ran no run to open and says why", () => {
    const [row] = failureRows([task("unstartable", null, { completed_at: minutesAgo(3) })]);
    expect(row?.runId).toBeNull();
    expect(row?.reason).toBe(NEVER_RAN);
    // A run-less failure is a start that never happened, not an abandoned
    // dependency, which the scheduler cancels; the row points at the agent
    // and the key, not at the graph.
    expect(NEVER_RAN).toMatch(/could not start it/);
    expect(NEVER_RAN).not.toMatch(/abandon/);
    expect(row?.failedAt).toBe(minutesAgo(3));
  });

  it("shows one line of the failure and keeps the rest for the title", () => {
    const [row] = failureRows([
      task("a", run("run-a", { failure: "provider error: 529\nretry after 30s", tainted: true })),
    ]);
    expect(row?.reason).toBe("provider error: 529 …");
    expect(row?.detail).toBe("provider error: 529\nretry after 30s");
    expect(row?.tainted).toBe(true);
  });
});

describe("toolRows", () => {
  it("leaves out tools nobody called", () => {
    const rows = toolRows([usage("used", { calls: 2, executed: 2 }), usage("idle")]);
    expect(rows.map((row) => row.tool)).toEqual(["used"]);
  });

  it("counts a refusal by the policy, a person or the budget alike", () => {
    const [row] = toolRows([usage("t", { calls: 6, executed: 2, denied: 3, approval_denied: 1 })]);
    expect(row?.refused).toBe(4);
  });

  it("marks a tool refused more often than it ran, and only that", () => {
    const rows = toolRows([
      usage("even", { calls: 4, executed: 2, denied: 2 }),
      usage("mostly", { calls: 3, executed: 1, denied: 1, approval_denied: 1 }),
      usage("never-ran", { calls: 1, executed: 0, denied: 1 }),
    ]);
    const marked = Object.fromEntries(rows.map((row) => [row.tool, row.mostlyRefused]));
    expect(marked).toEqual({ even: false, mostly: true, "never-ran": true });
  });

  it("puts mostly-refused tools first, then the busiest", () => {
    const rows = toolRows([
      usage("busy", { calls: 59, executed: 58, approval_denied: 1 }),
      usage("quiet", { calls: 4, executed: 4 }),
      usage("refused", { calls: 8, executed: 2, denied: 5, approval_denied: 1 }),
    ]);
    expect(rows.map((row) => row.tool)).toEqual(["refused", "busy", "quiet"]);
  });

  it("carries the calls that ran and failed through for the meter", () => {
    const [row] = toolRows([
      usage("t", { calls: 59, executed: 58, failed: 2, approval_denied: 1 }),
    ]);
    expect(row).toMatchObject({ executed: 58, failed: 2, refused: 1 });
  });

  it("keeps a tool the catalogue no longer offers, without a risk", () => {
    const [row] = toolRows([usage("retired", { calls: 1, executed: 1, risk: null })]);
    expect(row?.risk).toBeNull();
  });
});

describe("refusedTotal", () => {
  it("adds every refusal across the report", () => {
    expect(
      refusedTotal([
        usage("a", { denied: 3, approval_denied: 1 }),
        usage("b", { denied: 0, approval_denied: 2 }),
        usage("c"),
      ]),
    ).toBe(6);
    expect(refusedTotal([])).toBe(0);
  });
});

describe("chainTile", () => {
  it("holds a dash before the first answer, and says unknown if that answer failed", () => {
    expect(chainTile(null, false, false)).toEqual({ value: NOT_YET, label: "audit chain" });
    expect(chainTile(null, false, true).value).toBe("unknown");
  });

  it("says intact only for a current, complete, verifying answer", () => {
    expect(chainTile(health(), false, false)).toEqual({
      value: "intact",
      label: "audit chain",
      tone: "ok",
    });
  });

  it("does not leave an intact up once the check behind it has failed", () => {
    expect(chainTile(health(), true, true).value).toBe("unknown");
  });

  it("keeps a break up even after a later check failed", () => {
    expect(chainTile(health({ intact: false }), true, true)).toMatchObject({
      value: "broken",
      tone: "danger",
    });
  });

  it("never calls a chain intact while records went unwritten", () => {
    const tile = chainTile(health({ unrecorded: 3 }), false, false);
    expect(tile.value).not.toBe("intact");
    expect(tile).toEqual({
      value: "incomplete",
      label: "audit chain · 3 unwritten",
      tone: "danger",
    });
    expect(chainTile(health({ unrecorded: 3 }), true, true).value).toBe("incomplete");
  });

  it("lets a break outrank unwritten records", () => {
    expect(chainTile(health({ intact: false, unrecorded: 2 }), false, false).value).toBe("broken");
  });
});

describe("unrecordedNote", () => {
  it("says nothing when every record was written", () => {
    expect(unrecordedNote(null)).toBeNull();
    expect(unrecordedNote(health())).toBeNull();
  });

  it("counts what went unwritten", () => {
    expect(unrecordedNote(health({ unrecorded: 1 }))).toMatch(/^1 audit record could not/);
    expect(unrecordedNote(health({ unrecorded: 4 }))).toMatch(/^4 audit records could not/);
  });

  it("says the chain verifies only when it does", () => {
    expect(unrecordedNote(health({ unrecorded: 2 }))).toMatch(/The chain verifies/);
    const broken = unrecordedNote(health({ intact: false, unrecorded: 2 }));
    expect(broken).toMatch(/^2 audit records could not be written/);
    expect(broken).not.toMatch(/verifies/);
  });
});

describe("toolMeter", () => {
  it("sizes a bar against the busiest tool and splits it by how calls ended", () => {
    const meter = toolMeter(row({ calls: 59, executed: 58, failed: 2, refused: 1 }), 59);
    expect(meter.width).toBe(100);
    expect(meter.parts).toEqual([
      { kind: "ran", count: 56 },
      { kind: "failed", count: 2 },
      { kind: "refused", count: 1 },
    ]);
  });

  it("draws calls that neither ran nor were refused as a part of their own", () => {
    const meter = toolMeter(row({ calls: 5, executed: 1, refused: 2 }), 10);
    expect(meter.width).toBe(50);
    expect(meter.parts).toEqual([
      { kind: "ran", count: 1 },
      { kind: "refused", count: 2 },
      { kind: "other", count: 2 },
    ]);
  });

  it("never draws a tool with calls narrower than a sliver", () => {
    expect(toolMeter(row({ calls: 1, executed: 1 }), 500).width).toBe(2);
  });

  it("draws nothing for an empty list or a tool nobody called", () => {
    expect(toolMeter(row({ calls: 3, executed: 3 }), 0)).toEqual({ width: 0, parts: [] });
    expect(toolMeter(row({ calls: 0 }), 10)).toEqual({ width: 0, parts: [] });
  });
});
