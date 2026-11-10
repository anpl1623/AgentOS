import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../../../bindings/ApprovalView";
import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { RunSummary } from "../../../bindings/RunSummary";
import type { StepView } from "../../../bindings/StepView";
import { isIconName } from "../../../components/icons";
import { mergeEntries } from "./runModel";
import {
  HASH_PREFIX,
  allOpen,
  callIds,
  callLook,
  firstLine,
  liveTone,
  modeCounts,
  pendingApprovals,
  runElapsed,
  sentence,
  shortHash,
  stateIcon,
  stateTone,
  stepLook,
  taintedKeys,
  toolFamily,
} from "./traceLook";

const at = (minute: number) => new Date(2026, 9, 6, 10, minute).toISOString();

function execution(id: string, minute: number, extra: Partial<ExecutionView> = {}): ExecutionView {
  return {
    id,
    run_id: "run-1",
    tool: "filesystem.read",
    call_id: `call-${id}`,
    arguments: "{}",
    outcome: "success",
    executed: true,
    effect: "allow",
    risk: "low",
    tainted: false,
    approval_id: null,
    duration_ms: 10,
    error: null,
    started_at: at(minute),
    ...extra,
  };
}

function step(ordinal: number, minute: number, kind = "planning"): StepView {
  return {
    ordinal,
    kind,
    state: "planning",
    summary: `step ${ordinal}`,
    tool_execution_id: null,
    at: at(minute),
  };
}

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
    input_tokens: 0,
    output_tokens: 0,
    started_at: at(0),
    completed_at: at(10),
    ...extra,
  };
}

describe("toolFamily", () => {
  it("names every family the runtime has, with a drawn icon", () => {
    const families = [
      "filesystem.read",
      "browser.navigate",
      "terminal.exec",
      "network.request",
      "computer.click",
      "vision.describe",
      "github.pulls.merge",
      "agent.delegate",
    ].map(toolFamily);
    expect(families.map((family) => family.key)).toEqual([
      "filesystem",
      "browser",
      "terminal",
      "network",
      "computer",
      "vision",
      "github",
      "agent",
    ]);
    expect(families.every((family) => isIconName(family.icon))).toBe(true);
    // Distinct icons, so the column can be scanned.
    expect(new Set(families.map((family) => family.icon)).size).toBe(families.length);
  });

  it("keeps an unknown family's own word with a generic mark", () => {
    expect(toolFamily("calendar.create")).toEqual({
      key: "calendar",
      label: "Calendar",
      icon: "zap",
    });
    expect(toolFamily("")).toEqual({ key: "tool", label: "Tool", icon: "zap" });
  });

  it("does not take a prototype key for a family", () => {
    expect(toolFamily("constructor.x").icon).toBe("zap");
    expect(toolFamily("toString")).toMatchObject({ key: "tostring", icon: "zap" });
  });
});

describe("nodes", () => {
  it("tones a call as the timeline does", () => {
    expect(callLook(execution("a", 1))).toEqual({ icon: "folder", tone: "ok", label: "Filesystem" });
    const refused = execution("b", 1, { outcome: "denied", executed: false });
    expect(callLook(refused).tone).toBe("blocked");
    const failed = execution("c", 1, { outcome: "failed" });
    expect(callLook(failed).tone).toBe("danger");
  });

  it("gives a step that reports a call that call's look", () => {
    const refused = execution("b", 1, { tool: "browser.type", outcome: "denied", executed: false });
    expect(stepLook("tool_call", refused)).toEqual({
      icon: "window",
      tone: "blocked",
      label: "Tool call",
    });
    expect(stepLook("tool_call")).toEqual({ icon: "play", tone: "neutral", label: "Tool call" });
  });

  it("marks model turns, and the steps looked for after a failure", () => {
    expect(stepLook("planning").tone).toBe("info");
    expect(stepLook("recovery")).toMatchObject({ icon: "retry", tone: "warn" });
    expect(stepLook("approval")).toMatchObject({ icon: "approvals", tone: "warn" });
    expect(stepLook("something_new")).toEqual({
      icon: "info",
      tone: "neutral",
      label: "Something new",
    });
  });
});

describe("taintedKeys", () => {
  it("marks the first tainted call and everything after it", () => {
    const entries = mergeEntries(
      [step(1, 1), step(2, 4)],
      [execution("a", 2), execution("b", 3, { tainted: true }), execution("c", 5, { tainted: true })],
    );
    expect([...taintedKeys(entries)]).toEqual(["execution-b", "step-2", "execution-c"]);
  });

  it("marks nothing for a run that never read untrusted data", () => {
    expect(taintedKeys(mergeEntries([step(1, 1)], [execution("a", 2)])).size).toBe(0);
  });
});

describe("mode counts and opening calls", () => {
  const entries = mergeEntries([step(1, 1), step(2, 3)], [execution("a", 2), execution("b", 4)]);

  it("counts what each mode shows", () => {
    expect(modeCounts(entries)).toEqual({ timeline: 4, calls: 2, steps: 2 });
  });

  it("opens every call, and knows when all are open", () => {
    expect(callIds(entries)).toEqual(["a", "b"]);
    expect(allOpen(new Set(["a"]), ["a", "b"])).toBe(false);
    expect(allOpen(new Set(["a", "b"]), ["a", "b"])).toBe(true);
    expect(allOpen(new Set(), [])).toBe(false);
  });
});

describe("the run's header", () => {
  it("pulses only while the run is live, in attention while it waits on someone", () => {
    expect(liveTone("executing")).toBe("info");
    expect(liveTone("waiting_for_approval")).toBe("warn");
    expect(liveTone("cancelling")).toBe("warn");
    expect(liveTone("completed")).toBeNull();
    expect(liveTone("failed")).toBeNull();
  });

  it("is loud only for a run that needs a person", () => {
    expect(stateTone("waiting_for_approval")).toBe("warn");
    expect(stateTone("cancelling")).toBe("warn");
    expect(stateTone("failed")).toBe("danger");
    expect(stateTone("completed")).toBe("ok");
    expect(stateTone("cancelled")).toBe("accent");
    expect(stateTone("executing")).toBe("info");
    expect(stateTone("something_new")).toBe("info");
  });

  it("gives each outcome its own shape", () => {
    const states = ["waiting_for_approval", "cancelling", "failed", "completed", "executing"];
    const icons = states.map(stateIcon);
    expect(icons).toEqual(["approvals", "stop", "x", "check", "zap"]);
    expect(stateIcon("idle")).toBe("info");
  });

  it("times a run to its end, or to now while it is live", () => {
    expect(runElapsed(run(), 0)).toBe(10 * 60_000);
    const live = run({ state: "executing", completed_at: null });
    expect(runElapsed(live, Date.parse(at(4)))).toBe(4 * 60_000);
  });

  it("does not count to the present for a run that ended without saying when", () => {
    expect(runElapsed(run({ state: "failed", completed_at: null }), Date.parse(at(50)))).toBeNull();
    expect(runElapsed(run({ started_at: "nope" }), 0)).toBeNull();
  });

  it("finds the approvals still waiting", () => {
    const approvals = [
      { id: "a", status: "approved" },
      { id: "b", status: "pending" },
    ] as ApprovalView[];
    expect(pendingApprovals(approvals).map((approval) => approval.id)).toEqual(["b"]);
  });
});

describe("text", () => {
  it("cuts a hash and marks the cut", () => {
    expect(shortHash("a".repeat(HASH_PREFIX))).toBe("a".repeat(HASH_PREFIX));
    expect(shortHash("b".repeat(HASH_PREFIX + 1))).toBe(`${"b".repeat(HASH_PREFIX)}…`);
  });

  it("keeps the first line of an error and says more follows", () => {
    expect(firstLine("  one line ")).toBe("one line");
    expect(firstLine("first\nsecond")).toBe("first …");
  });

  it("writes a wire value as chip text", () => {
    expect(sentence("waiting_for_approval")).toBe("Waiting for approval");
  });
});
