import { describe, expect, it } from "vitest";

import type { RunSummary } from "../bindings/RunSummary";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { TaskNodeView } from "../bindings/TaskNodeView";
import type { TaskSummary } from "../bindings/TaskSummary";
import {
  BLOCKER_CHARS,
  NO_FILTER,
  attemptNote,
  countNote,
  dependencyChoices,
  distinctOptions,
  emptyListMessage,
  filterTasks,
  heldUntil,
  isFiltering,
  queueRows,
  schedulerNote,
  showsStatusWithoutRun,
  truncate,
} from "./tasksModel";

function run(extra: Partial<RunSummary> = {}): RunSummary {
  return {
    id: "run-1",
    attempt: 1,
    state: "completed",
    tainted: false,
    taint_sources: [],
    steps: 3,
    result: null,
    failure: null,
    input_tokens: 10,
    output_tokens: 5,
    started_at: "2026-10-06T10:00:00Z",
    completed_at: "2026-10-06T10:01:00Z",
    ...extra,
  };
}

function task(id: string, extra: Partial<TaskSummary> = {}): TaskSummary {
  return {
    id,
    objective: `Objective ${id}`,
    status: "succeeded",
    agent_name: "sales",
    agent_id: "agent-1",
    created_at: "2026-10-06T10:00:00Z",
    completed_at: null,
    latest_run: run(),
    ...extra,
  };
}

function node(id: string, extra: Partial<TaskNodeView> = {}): TaskNodeView {
  return {
    id,
    objective: `Objective ${id}`,
    status: "pending",
    agent_id: "agent-1",
    agent_name: "sales",
    scheduled_for: null,
    schedule_id: null,
    created_at: "2026-10-06T10:00:00Z",
    blocked_by: [],
    blocks: [],
    runnable: false,
    unreachable: false,
    blocked_by_failure: null,
    ...extra,
  };
}

function scheduler(extra: Partial<SchedulerView> = {}): SchedulerView {
  return {
    running: false,
    tick_seconds: 30,
    max_concurrent_runs: 1,
    started_at: null,
    error: null,
    active_schedules: 0,
    next_fire_at: null,
    overdue: 0,
    runnable_tasks: 0,
    unreachable_tasks: 0,
    ...extra,
  };
}

describe("filterTasks", () => {
  const tasks = [
    task("a", { objective: "Draft the follow-ups", agent_name: "sales", status: "failed" }),
    task("b", { objective: "Archive old logs", agent_name: "ops", status: "succeeded" }),
    task("c", { objective: "Summarise the week", agent_name: "sales", status: "succeeded" }),
  ];

  it("keeps everything when nothing narrows it", () => {
    expect(filterTasks(tasks, NO_FILTER)).toEqual(tasks);
    expect(isFiltering(NO_FILTER)).toBe(false);
  });

  it("matches text against the objective and the agent, ignoring case", () => {
    expect(filterTasks(tasks, { ...NO_FILTER, text: "ARCHIVE" }).map((t) => t.id)).toEqual(["b"]);
    expect(filterTasks(tasks, { ...NO_FILTER, text: "ops" }).map((t) => t.id)).toEqual(["b"]);
  });

  it("does not treat surrounding whitespace as a filter", () => {
    expect(isFiltering({ ...NO_FILTER, text: "   " })).toBe(false);
    expect(filterTasks(tasks, { ...NO_FILTER, text: "  " })).toHaveLength(3);
  });

  it("combines agent, status and text", () => {
    const filter = { text: "the", agent: "sales", status: "succeeded" };
    expect(filterTasks(tasks, filter).map((t) => t.id)).toEqual(["c"]);
    expect(isFiltering(filter)).toBe(true);
  });
});

describe("toolbar options and counts", () => {
  it("lists distinct values sorted, keeping a selection the page no longer holds", () => {
    expect(distinctOptions(["sales", "ops", "sales"], "")).toEqual(["ops", "sales"]);
    expect(distinctOptions(["sales"], "ops")).toEqual(["ops", "sales"]);
  });

  it("says shown of total only while filtering", () => {
    expect(countNote(2, 50, true)).toBe("2 of 50");
    expect(countNote(50, 50, false)).toBeNull();
  });

  it("does not call a filtered-out list an empty history", () => {
    expect(emptyListMessage(true)).not.toMatch(/Nothing has been run/);
    expect(emptyListMessage(false)).toMatch(/Nothing has been run/);
  });
});

describe("rows in Recent", () => {
  it("names the attempt only once a task has been retried", () => {
    expect(attemptNote(task("a", { latest_run: run({ attempt: 1 }) }))).toBeNull();
    expect(attemptNote(task("a", { latest_run: run({ attempt: 3 }) }))).toBe("attempt 3");
    expect(attemptNote(task("a", { latest_run: null }))).toBeNull();
  });

  it("keeps the status beside never ran when the task ended without running", () => {
    expect(showsStatusWithoutRun(task("a", { status: "pending", latest_run: null }))).toBe(false);
    expect(showsStatusWithoutRun(task("a", { status: "blocked", latest_run: null }))).toBe(false);
    expect(showsStatusWithoutRun(task("a", { status: "failed", latest_run: null }))).toBe(true);
    expect(showsStatusWithoutRun(task("a", { status: "cancelled", latest_run: null }))).toBe(true);
  });
});

describe("queueRows", () => {
  it("lists only pending and blocked work", () => {
    const graph = [
      node("a", { status: "pending" }),
      node("b", { status: "blocked" }),
      node("c", { status: "running" }),
      node("d", { status: "succeeded" }),
      node("e", { status: "failed" }),
      node("f", { status: "cancelled" }),
    ];
    expect(queueRows(graph).map((row) => row.id)).toEqual(["a", "b"]);
  });

  it("names each blocker by its objective, cut short, and by id when outside the read", () => {
    const long = "Collect every reply from the inbox and the shared mailbox since Monday";
    const graph = [
      node("gather", { status: "running", objective: long }),
      node("sum", { status: "blocked", blocked_by: ["gather", "elsewhere"] }),
    ];
    const [row] = queueRows(graph);
    expect(row?.waitsFor).toEqual([
      { id: "gather", label: truncate(long, BLOCKER_CHARS), full: long },
      { id: "elsewhere", label: "elsewhere", full: "elsewhere" },
    ]);
    expect(row?.waitsFor[0]?.label.length).toBeLessThanOrEqual(BLOCKER_CHARS);
    expect(row?.waitsFor[0]?.label.endsWith("…")).toBe(true);
  });

  it("marks a runnable task ready and a dead branch unreachable with its culprit", () => {
    const graph = [
      node("failed", { status: "failed", objective: "Collect the replies." }),
      node("ready", { runnable: true }),
      node("dead", {
        status: "blocked",
        blocked_by: ["failed"],
        unreachable: true,
        blocked_by_failure: "failed",
      }),
      node("waiting", { status: "blocked", blocked_by: ["ready"] }),
    ];
    const rows = queueRows(graph);
    expect(rows.map((row) => [row.id, row.state, row.culprit])).toEqual([
      ["ready", "ready", null],
      ["dead", "unreachable", "Collect the replies."],
      ["waiting", "waiting", null],
    ]);
  });

  it("never calls a task ready when it is also reported unreachable", () => {
    const [row] = queueRows([node("x", { runnable: true, unreachable: true })]);
    expect(row?.state).toBe("unreachable");
  });

  it("carries the moment a task is held until", () => {
    const [row] = queueRows([node("x", { scheduled_for: "2026-10-07T09:00:00Z" })]);
    expect(row?.heldUntil).toBe("2026-10-07T09:00:00Z");
  });
});

describe("heldUntil", () => {
  it("gives the clock alone today, and the date as well on another day", () => {
    const now = new Date(2026, 9, 6, 8, 0).getTime();
    const later = new Date(2026, 9, 6, 17, 30).toISOString();
    const tomorrow = new Date(2026, 9, 7, 9, 0).toISOString();
    expect(heldUntil(later, now)).toMatch(/^held until \d/);
    expect(heldUntil(tomorrow, now)).toMatch(/7/);
    expect(heldUntil(tomorrow, now).length).toBeGreaterThan(heldUntil(later, now).length);
  });

  it("prints an unreadable time as written rather than as Invalid Date", () => {
    expect(heldUntil("soon", 0)).toBe("held until soon");
  });
});

describe("dependencyChoices", () => {
  const graph = [
    node("a", { status: "pending" }),
    node("b", { status: "blocked", blocked_by: ["a"] }),
    node("c", { status: "running" }),
    node("d", { status: "succeeded" }),
    node("e", { status: "failed" }),
  ];

  it("offers only work that is still to be done", () => {
    expect(dependencyChoices(graph).map((choice) => choice.id)).toEqual(["a", "b", "c"]);
  });

  it("leaves out a task that will never start", () => {
    const doomed = [
      node("dead", { status: "failed" }),
      node("stuck", { status: "blocked", blocked_by: ["dead"], unreachable: true }),
      node("live", { status: "pending" }),
    ];
    expect(dependencyChoices(doomed).map((choice) => choice.id)).toEqual(["live"]);
  });

  it("leaves out the task itself and what it already waits for", () => {
    expect(dependencyChoices(graph, graph[1]!).map((choice) => choice.id)).toEqual(["c"]);
  });

  it("still offers a choice that would close a cycle, so the runtime can say why not", () => {
    // `a` is waited for by `b`; making `a` wait for `b` is a cycle the
    // runtime refuses with its path. Hiding it would hide the explanation.
    expect(dependencyChoices(graph, graph[0]!).map((choice) => choice.id)).toContain("b");
  });
});

describe("schedulerNote", () => {
  it("says nothing while the scheduler runs, or before it has been read", () => {
    expect(schedulerNote(scheduler({ running: true }))).toBeNull();
    expect(schedulerNote(null)).toBeNull();
  });

  it("says queued work will not start on its own while it is off", () => {
    expect(schedulerNote(scheduler())).toMatch(/will not start on their own/);
  });

  it("gives the reason when the scheduler stopped by itself", () => {
    const note = schedulerNote(scheduler({ error: "another process holds the scheduler lease" }));
    expect(note).toMatch(/another process holds the scheduler lease/);
    expect(note).toMatch(/will not start on their own/);
  });
});

describe("truncate", () => {
  it("leaves short text alone and marks what it cut", () => {
    expect(truncate("  short  ", 10)).toBe("short");
    expect(truncate("abcdefghij", 5)).toBe("abcd…");
    expect(truncate("abcdefghij", 5)).toHaveLength(5);
  });
});
