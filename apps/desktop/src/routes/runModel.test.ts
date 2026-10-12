import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApprovalView } from "../bindings/ApprovalView";
import type { ExecutionView } from "../bindings/ExecutionView";
import type { RunSummary } from "../bindings/RunSummary";
import type { StepView } from "../bindings/StepView";
import type { TraceView } from "../bindings/TraceView";
import {
  REFRESH_DEBOUNCE_MS,
  REFRESH_MAX_WAIT_MS,
  TAINT_LABEL,
  approvalTone,
  attemptsLabel,
  buildTimeline,
  canRetry,
  coalesce,
  concernsRun,
  entriesFor,
  failureAnchor,
  hhmmss,
  marker,
  mergeEntries,
  runWindow,
  shownState,
  toolLook,
  transcript,
  unionLength,
  waitingSummary,
} from "./runModel";

/** A local wall-clock time on the fixture day, as the runtime would write it. */
function at(minute: number, second = 0): string {
  return new Date(2026, 9, 6, 10, minute, second).toISOString();
}

const ms = (iso: string) => Date.parse(iso);

function run(extra: Partial<RunSummary> = {}): RunSummary {
  return {
    id: "run-1",
    attempt: 1,
    state: "completed",
    tainted: false,
    taint_sources: [],
    steps: 0,
    result: null,
    failure: null,
    input_tokens: 1200,
    output_tokens: 340,
    started_at: at(0),
    completed_at: at(10),
    ...extra,
  };
}

function step(
  ordinal: number,
  kind: string,
  minute: number,
  extra: Partial<StepView> = {},
): StepView {
  return {
    ordinal,
    kind,
    state: "planning",
    summary: `step ${ordinal}`,
    tool_execution_id: null,
    at: at(minute),
    ...extra,
  };
}

function execution(id: string, minute: number, extra: Partial<ExecutionView> = {}): ExecutionView {
  return {
    id,
    run_id: "run-1",
    tool: "browser.navigate",
    call_id: `call-${id}`,
    arguments: "{}",
    outcome: "success",
    executed: true,
    effect: "allow",
    risk: "low",
    tainted: false,
    approval_id: null,
    duration_ms: 1_000,
    error: null,
    started_at: at(minute),
    ...extra,
  };
}

function approval(
  id: string,
  from: number,
  to: number | null,
  extra: Partial<ApprovalView> = {},
): ApprovalView {
  return {
    id,
    agent_name: "sales",
    task_id: "task-1",
    run_id: "run-1",
    objective: "o",
    tool: "browser.type",
    arguments: "{}",
    risk: "high",
    reason: "r",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: null,
    explanation: "",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: to === null ? "pending" : "approved",
    requested_at: at(from),
    decided_at: to === null ? null : at(to),
    note: null,
    ...extra,
  };
}

function trace(extra: Partial<TraceView> = {}): TraceView {
  return {
    run: run(),
    task_id: "task-1",
    agent_name: "sales",
    objective: "Find the overdue follow-ups.",
    steps: [],
    executions: [],
    approvals: [],
    ...extra,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("concernsRun", () => {
  it("matches an event for this run and nothing else", () => {
    expect(concernsRun({ run_id: "run-1", kind: "x" }, "run-1")).toBe(true);
    expect(concernsRun({ run_id: "run-2" }, "run-1")).toBe(false);
    expect(concernsRun({ run_id: null }, "run-1")).toBe(false);
  });

  it("does not trust the payload's shape", () => {
    expect(concernsRun(null, "run-1")).toBe(false);
    expect(concernsRun("run-1", "run-1")).toBe(false);
    expect(concernsRun(undefined, "run-1")).toBe(false);
  });
});

describe("coalesce", () => {
  it("turns a burst of events into one reload after the burst", () => {
    vi.useFakeTimers();
    const reload = vi.fn();
    const refresh = coalesce(reload, REFRESH_DEBOUNCE_MS);
    // A model turn: step, execution, permission, approval, a few ms apart.
    for (let i = 0; i < 5; i += 1) {
      refresh.schedule();
      vi.advanceTimersByTime(10);
    }
    expect(reload).not.toHaveBeenCalled();
    vi.advanceTimersByTime(REFRESH_DEBOUNCE_MS);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("waits for the last event, not the first", () => {
    vi.useFakeTimers();
    const reload = vi.fn();
    const refresh = coalesce(reload, 150);
    refresh.schedule();
    vi.advanceTimersByTime(100);
    refresh.schedule();
    vi.advanceTimersByTime(50);
    expect(reload).not.toHaveBeenCalled();
    vi.advanceTimersByTime(99);
    expect(reload).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads during a burst that never pauses", () => {
    vi.useFakeTimers();
    const reload = vi.fn();
    const refresh = coalesce(reload, REFRESH_DEBOUNCE_MS, REFRESH_MAX_WAIT_MS);
    // An event every 100ms, inside the debounce every time, for three seconds.
    for (let t = 0; t < 3_000; t += 100) {
      refresh.schedule();
      vi.advanceTimersByTime(100);
    }
    expect(reload.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(reload.mock.calls.length).toBeLessThanOrEqual(3);
  });

  it("runs at once when flushed, in place of what was pending", () => {
    vi.useFakeTimers();
    const reload = vi.fn();
    const refresh = coalesce(reload, REFRESH_DEBOUNCE_MS);
    refresh.schedule();
    refresh.flush();
    expect(reload).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(REFRESH_DEBOUNCE_MS * 2);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads again for a later burst, and not at all once cancelled", () => {
    vi.useFakeTimers();
    const reload = vi.fn();
    const refresh = coalesce(reload, REFRESH_DEBOUNCE_MS);
    refresh.schedule();
    vi.advanceTimersByTime(REFRESH_DEBOUNCE_MS);
    refresh.schedule();
    vi.advanceTimersByTime(REFRESH_DEBOUNCE_MS);
    expect(reload).toHaveBeenCalledTimes(2);
    refresh.schedule();
    refresh.cancel();
    vi.advanceTimersByTime(REFRESH_DEBOUNCE_MS * 2);
    expect(reload).toHaveBeenCalledTimes(2);
  });
});

describe("mergeEntries", () => {
  it("interleaves steps and calls by when they happened", () => {
    const entries = mergeEntries(
      [step(1, "planning", 0), step(2, "planning", 3)],
      [execution("a", 1), execution("b", 4)],
    );
    expect(entries.map((entry) => entry.key)).toEqual([
      "step-1",
      "execution-a",
      "step-2",
      "execution-b",
    ]);
  });

  it("puts a call before the step that reports it when they share an instant", () => {
    const entries = mergeEntries(
      [step(2, "tool_call", 1, { tool_execution_id: "a" })],
      [execution("a", 1)],
    );
    expect(entries.map((entry) => entry.kind)).toEqual(["execution", "step"]);
  });

  it("breaks a tie between steps by ordinal, not by the order given", () => {
    const entries = mergeEntries([step(3, "planning", 2), step(2, "planning", 2)], []);
    expect(entries.map((entry) => entry.key)).toEqual(["step-2", "step-3"]);
  });

  it("orders by parsed time, not by the text of the timestamp", () => {
    // The same instant in two spellings, and a later one that sorts earlier as text.
    const entries = mergeEntries(
      [
        step(1, "planning", 0, { at: "2026-10-06T10:00:09.5+00:00" }),
        step(2, "planning", 0, { at: "2026-10-06T10:00:10Z" }),
        step(3, "planning", 0, { at: "2026-10-06T09:00:11-02:00" }),
      ],
      [],
    );
    expect(entries.map((entry) => entry.key)).toEqual(["step-1", "step-2", "step-3"]);
  });

  it("sorts a time it cannot read last instead of scrambling the rest", () => {
    const entries = mergeEntries(
      [step(1, "planning", 0, { at: "garbage" }), step(2, "planning", 5), step(3, "planning", 1)],
      [],
    );
    expect(entries.map((entry) => entry.key)).toEqual(["step-3", "step-2", "step-1"]);
  });

  it("keeps every entry in each mode's view, and only its own kind", () => {
    const entries = mergeEntries([step(1, "planning", 0)], [execution("a", 1)]);
    expect(entriesFor("timeline", entries)).toHaveLength(2);
    expect(entriesFor("calls", entries).map((entry) => entry.kind)).toEqual(["execution"]);
    expect(entriesFor("steps", entries).map((entry) => entry.kind)).toEqual(["step"]);
  });
});

describe("marker", () => {
  it("gives every kind a label as well as a glyph", () => {
    expect(marker("tool_call")).toEqual({ glyph: "▶", label: "tool call" });
    expect(marker("toolcall").label).toBe("tool call");
    expect(marker("approval").label).toBe("approval");
    expect(marker("verification").label).toBe("verification");
    expect(marker("recovery").label).toBe("recovery");
    expect(marker("planning").label).toBe("step");
    expect(marker("something new").label).toBe("step");
  });
});

describe("the run's state and actions", () => {
  it("shows cancelling, not cancelled, until the runtime says the run ended", () => {
    expect(shownState("executing", true)).toBe("cancelling");
    expect(shownState("waiting_for_approval", true)).toBe("cancelling");
    expect(shownState("executing", false)).toBe("executing");
    // Reconciled: once the runtime reports the end, its word stands.
    expect(shownState("cancelled", true)).toBe("cancelled");
    expect(shownState("completed", true)).toBe("completed");
  });

  it("offers Retry only on a failed or cancelled latest attempt", () => {
    expect(canRetry(run({ state: "failed" }), "run-1")).toBe(true);
    expect(canRetry(run({ state: "cancelled" }), null)).toBe(true);
    expect(canRetry(run({ state: "completed" }), "run-1")).toBe(false);
    expect(canRetry(run({ state: "executing" }), "run-1")).toBe(false);
    // An older attempt: the runtime would retry the latest, not this one.
    expect(canRetry(run({ state: "failed" }), "run-2")).toBe(false);
  });

  it("names the attempt among several, and says nothing about a lone one", () => {
    const first = run({ id: "a", attempt: 1 });
    const second = run({ id: "b", attempt: 2 });
    expect(attemptsLabel(first, [first, second])).toBe("attempt 1 of 2");
    expect(attemptsLabel(first, [first])).toBeNull();
  });

  it("tones a pending approval as live work and a decided one by its decision", () => {
    expect(approvalTone("pending")).toBe("live");
    expect(approvalTone("approved")).toBe("ok");
    expect(approvalTone("denied")).toBe("blocked");
    expect(approvalTone("expired")).toBe("neutral");
  });
});

describe("failureAnchor", () => {
  it("pins a tool failure to that tool's failing call, preferring the matching error", () => {
    const executions = [
      execution("a", 1, { tool: "filesystem.write", outcome: "failed", error: "disk full" }),
      execution("b", 2, { tool: "filesystem.write", outcome: "failed", error: "read-only file" }),
      execution("c", 3, { tool: "filesystem.write", outcome: "success" }),
    ];
    const failed = run({ state: "failed", failure: "tool `filesystem.write` failed: disk full" });
    expect(failureAnchor(failed, [], executions)).toEqual({ kind: "execution", id: "a" });
  });

  it("falls back to the tool's last failing call when no error matches", () => {
    const executions = [
      execution("a", 1, { tool: "terminal.exec", outcome: "timed_out", error: null }),
    ];
    const failed = run({ failure: "tool `terminal.exec` failed: exited 137" });
    expect(failureAnchor(failed, [], executions)).toEqual({ kind: "execution", id: "a" });
  });

  it("pins a recovered-from failure to the recovery step that quotes it", () => {
    const failure = "provider error: anthropic returned 529 (overloaded)";
    const steps = [
      step(1, "planning", 0),
      step(2, "recovery", 1, { summary: `Recovering from: ${failure}` }),
      step(3, "recovery", 2, { summary: `Recovering from: ${failure}` }),
    ];
    expect(failureAnchor(run({ failure }), steps, [])).toEqual({ kind: "step", ordinal: 3 });
  });

  it("guesses nothing when the trace did not produce the failure", () => {
    const failed = run({ failure: "the process exited while this run was in progress" });
    expect(failureAnchor(failed, [step(1, "planning", 0)], [execution("a", 1)])).toBeNull();
    expect(failureAnchor(run({ failure: null }), [], [])).toBeNull();
  });
});

describe("toolLook", () => {
  it("tones by whether the call executed, and hatches one that did not", () => {
    expect(toolLook(execution("a", 0))).toEqual({ tone: "ok", hatch: null });
    expect(toolLook(execution("a", 0, { outcome: "failed" }))).toEqual({
      tone: "danger",
      hatch: null,
    });
    expect(toolLook(execution("a", 0, { outcome: "denied", executed: false }))).toEqual({
      tone: "blocked",
      hatch: "refused",
    });
    expect(toolLook(execution("a", 0, { outcome: "approval_denied", executed: false }))).toEqual({
      tone: "blocked",
      hatch: "refused",
    });
    expect(toolLook(execution("a", 0, { outcome: "invalid_arguments", executed: false }))).toEqual(
      { tone: "danger", hatch: "skipped" },
    );
  });
});

describe("waiting time", () => {
  it("counts overlapping waits once", () => {
    expect(
      unionLength([
        { from: 0, to: 10 },
        { from: 5, to: 15 },
        { from: 20, to: 25 },
        { from: 21, to: 22 },
      ]),
    ).toBe(20);
  });

  it("counts touching waits as one and ignores empty ones", () => {
    expect(unionLength([{ from: 0, to: 5 }, { from: 5, to: 8 }, { from: 9, to: 9 }])).toBe(8);
    expect(unionLength([])).toBe(0);
  });

  it("says plainly how much of the run was spent waiting", () => {
    expect(waitingSummary(60_000, 660_000)).toBe(
      "1m of the run's 11m was spent waiting for a decision.",
    );
    expect(waitingSummary(0, 660_000)).toBe("The run did not wait for a decision.");
  });
});

describe("runWindow", () => {
  it("runs from start to completion", () => {
    expect(runWindow(trace(), 0)).toEqual({ start: ms(at(0)), end: ms(at(10)) });
  });

  it("runs to now while the run is live", () => {
    const live = trace({ run: run({ state: "executing", completed_at: null }) });
    expect(runWindow(live, ms(at(4)))).toEqual({ start: ms(at(0)), end: ms(at(4)) });
  });

  it("ends a finished run with no recorded end at the last thing it recorded", () => {
    const ended = trace({
      run: run({ state: "failed", completed_at: null }),
      executions: [execution("a", 2, { duration_ms: 30_000 })],
    });
    expect(runWindow(ended, ms(at(50)))?.end).toBe(ms(at(2, 30)));
  });

  it("has no window when the start cannot be read or nothing elapsed", () => {
    expect(runWindow(trace({ run: run({ started_at: "nope" }) }), 0)).toBeNull();
    expect(runWindow(trace({ run: run({ completed_at: at(0) }) }), 0)).toBeNull();
  });
});

describe("buildTimeline", () => {
  it("draws nothing for a run with no entries or no duration", () => {
    expect(buildTimeline(trace(), 0)).toBeNull();
    const instant = trace({
      run: run({ completed_at: at(0) }),
      steps: [step(1, "planning", 0)],
    });
    expect(buildTimeline(instant, 0)).toBeNull();
  });

  it("runs each model turn to the next step of any kind, and the last to the end", () => {
    const timeline = buildTimeline(
      trace({
        steps: [
          step(1, "planning", 1),
          step(2, "tool_call", 3),
          step(3, "verification", 6),
        ],
      }),
      0,
    );
    expect(timeline?.model.map((span) => [span.from, span.to])).toEqual([
      [ms(at(1)), ms(at(3))],
      [ms(at(6)), ms(at(10))],
    ]);
    expect(timeline?.model[0]?.target).toEqual({ kind: "step", ordinal: 1 });
  });

  it("times each call as the runtime did and points at its row", () => {
    const timeline = buildTimeline(
      trace({ executions: [execution("a", 2, { duration_ms: 1_500 })] }),
      0,
    );
    expect(timeline?.tools[0]).toMatchObject({
      from: ms(at(2)),
      to: ms(at(2)) + 1_500,
      tone: "ok",
      hatch: null,
      target: { kind: "execution", id: "a" },
    });
  });

  it("marks the first call made after the run read untrusted data", () => {
    const timeline = buildTimeline(
      trace({
        executions: [
          execution("c", 5, { tainted: true }),
          execution("a", 1),
          execution("b", 3, { tainted: true }),
        ],
      }),
      0,
    );
    expect(timeline?.taint).toEqual({ at: ms(at(3)), label: TAINT_LABEL });
    expect(TAINT_LABEL).toBe("read untrusted data");
  });

  it("counts overlapping approvals once and runs an undecided one to the end", () => {
    const timeline = buildTimeline(
      trace({
        approvals: [approval("a", 1, 4), approval("b", 2, 3), approval("c", 8, null)],
      }),
      0,
    );
    expect(timeline?.waiting.map((span) => [span.from, span.to])).toEqual([
      [ms(at(1)), ms(at(4))],
      [ms(at(2)), ms(at(3))],
      [ms(at(8)), ms(at(10))],
    ]);
    expect(timeline?.waitingMs).toBe(5 * 60_000);
    expect(timeline?.summary).toBe("5m of the run's 10m was spent waiting for a decision.");
    const looks = timeline?.waiting.map((span) => [span.tone, span.hatch]);
    expect(looks).toEqual([
      ["warn", "waiting"],
      ["warn", "waiting"],
      ["warn", "waiting"],
    ]);
  });

  it("clips what falls outside the run instead of stretching the chart", () => {
    const timeline = buildTimeline(
      trace({
        approvals: [approval("a", -5, 2)],
        executions: [execution("x", 9, { duration_ms: 600_000 })],
      }),
      0,
    );
    expect(timeline?.waiting[0]?.from).toBe(ms(at(0)));
    expect(timeline?.tools[0]?.to).toBe(ms(at(10)));
    expect(timeline?.waitingMs).toBe(2 * 60_000);
  });
});

describe("transcript", () => {
  it("gives the run's facts, then one line per entry with times", () => {
    const text = transcript(
      trace({
        run: run({
          state: "failed",
          tainted: true,
          taint_sources: ["web:http://127.0.0.1:8420/customers/globex"],
          failure: "tool `filesystem.read` failed: permission denied",
        }),
        steps: [step(1, "planning", 0, { summary: "I will read the notes." })],
        executions: [
          execution("a", 1, {
            tool: "filesystem.read",
            effect: "deny",
            outcome: "denied",
            executed: false,
            error: "permission denied: no rule matched",
          }),
        ],
      }),
    );
    expect(text).toBe(
      [
        "Objective: Find the overdue follow-ups.",
        "Agent: sales",
        "Run: run-1",
        "Attempt: 1",
        "State: failed",
        "Taint: read untrusted data from web:http://127.0.0.1:8420/customers/globex",
        "Tokens: 1200 in, 340 out",
        "Failure: tool `filesystem.read` failed: permission denied",
        "",
        `${hhmmss(at(0))} planning I will read the notes.`,
        `${hhmmss(at(1))} tool filesystem.read decision=deny outcome=denied`,
        "    permission denied: no rule matched",
        "",
      ].join("\n"),
    );
  });

  it("writes the state the screen shows, so a pending stop is not copied as stopped", () => {
    const text = transcript(trace({ run: run({ state: "executing" }) }), "cancelling");
    expect(text).toMatch(/^State: cancelling$/m);
  });

  it("indents every line of a multi-line error and summary under its entry", () => {
    const text = transcript(
      trace({
        steps: [step(1, "planning", 0, { summary: "first\nsecond" })],
        executions: [execution("a", 1, { error: "line one\nline two", outcome: "failed" })],
      }),
    );
    expect(text).toContain(`${hhmmss(at(0))} planning first\n    second`);
    expect(text).toContain("\n    line one\n    line two");
  });

  it("writes direction overrides and invisible characters as visible escapes", () => {
    const text = transcript(
      trace({ steps: [step(1, "planning", 0, { summary: "rm -rf ‮fdp.txt​" })] }),
    );
    expect(text).not.toMatch(/[‮​]/);
    expect(text).toContain("rm -rf ⟨U+202E⟩fdp.txt⟨U+200B⟩");
  });

  it("says a run read nothing untrusted rather than leaving the line out", () => {
    expect(transcript(trace())).toMatch(/^Taint: none$/m);
  });
});

describe("hhmmss", () => {
  it("is a fixed 24-hour clock, whatever the locale", () => {
    expect(hhmmss(new Date(2026, 9, 6, 7, 5, 9).toISOString())).toBe("07:05:09");
    expect(hhmmss(new Date(2026, 9, 6, 23, 59, 0).toISOString())).toBe("23:59:00");
  });

  it("gives an unreadable time back as written", () => {
    expect(hhmmss("not a time")).toBe("not a time");
  });
});
