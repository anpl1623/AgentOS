import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { idleSelection } from "./approvalsModel";
import {
  budgetMeter,
  matchedRule,
  queueItemLabel,
  queueSummary,
  recentTally,
  riskStatTone,
  riskWord,
  ruleEffect,
  shownId,
  tallyLine,
  taintOrigin,
  toolIcon,
} from "./deskModel";

function approval(id: string, extra: Partial<ApprovalView> = {}): ApprovalView {
  return {
    id,
    agent_name: "sales",
    delegation_chain: [],
    task_id: "task",
    run_id: "run",
    objective: "follow up",
    tool: "email.send",
    arguments: "{}",
    risk: "low",
    reason: "sends mail",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: 10,
    explanation: "Sends one email.",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: "2026-10-01T10:00:00Z",
    decided_at: null,
    note: null,
    ...extra,
  };
}

describe("the request the desk shows", () => {
  const order = ["a", "b", "c"];

  it("is the most dangerous request while nothing is selected", () => {
    expect(shownId(idleSelection, order)).toBe("a");
    expect(shownId(idleSelection, [])).toBeNull();
  });

  it("is the selected request", () => {
    expect(shownId({ selected: "c", intent: null }, order)).toBe("c");
  });

  it("falls back to the card holding a half-made decision, then to the first", () => {
    const intent = { id: "b", kind: "approve" as const, note: "" };
    // The selected card has left; the armed one is still waiting.
    expect(shownId({ selected: "gone", intent }, order)).toBe("b");
    expect(shownId({ selected: "gone", intent: { ...intent, id: "also-gone" } }, order)).toBe("a");
  });
});

describe("the figures over the queue", () => {
  const now = Date.parse("2026-10-01T10:30:00Z");

  it("count the queue, its taint and its worst risk, and time the oldest request", () => {
    const summary = queueSummary(
      [
        approval("a", { risk: "medium", requested_at: "2026-10-01T10:20:00Z", tainted: true }),
        approval("b", {
          risk: "high",
          requested_at: "2026-10-01T10:05:00Z",
          tainted: true,
          effect_before_taint: "allow",
        }),
        approval("c", { risk: "low", requested_at: "2026-10-01T10:28:00Z" }),
      ],
      now,
    );
    expect(summary).toEqual({
      waiting: 3,
      tainted: 2,
      onlyTaint: 1,
      highest: "high",
      oldest: "25m",
    });
  });

  it("say nothing about risk or age for an empty queue", () => {
    expect(queueSummary([], now)).toEqual({
      waiting: 0,
      tainted: 0,
      onlyTaint: 0,
      highest: null,
      oldest: null,
    });
  });

  it("rank a level this build does not know above critical, and do not let a bad time be oldest", () => {
    const summary = queueSummary(
      [
        approval("a", { risk: "critical", requested_at: "garbled" }),
        approval("b", { risk: "catastrophic", requested_at: "2026-10-01T10:29:00Z" }),
      ],
      now,
    );
    expect(summary.highest).toBe("catastrophic");
    expect(summary.oldest).toBe("1m");
  });

  it("tone the risk tile only when the risk asks for attention", () => {
    expect(riskStatTone(null)).toBe("accent");
    expect(riskStatTone("none")).toBe("accent");
    expect(riskStatTone("low")).toBe("accent");
    expect(riskStatTone("medium")).toBe("warn");
    expect(riskStatTone("high")).toBe("danger");
    expect(riskStatTone("critical")).toBe("critical");
    expect(riskStatTone("catastrophic")).toBe("critical");
    expect(riskWord("high")).toBe("High");
  });
});

describe("the approval budget meter", () => {
  it("fills with the requests asked, and warns near the end", () => {
    expect(budgetMeter(3, null)).toBeNull();
    expect(budgetMeter(3, 10)).toEqual({ fill: 0.3, tone: "calm" });
    expect(budgetMeter(8, 10)).toEqual({ fill: 0.8, tone: "warn" });
  });

  it("is spent once this is the last request the run may make", () => {
    expect(budgetMeter(10, 10)).toEqual({ fill: 1, tone: "spent" });
    expect(budgetMeter(12, 10)).toEqual({ fill: 1, tone: "spent" });
    expect(budgetMeter(0, 0)).toEqual({ fill: 1, tone: "spent" });
  });
});

describe("the rule that matched", () => {
  it("is read from a reason in the permission engine's form", () => {
    expect(matchedRule("rule `browser.interact` matched; escalated because …")).toBe(
      "browser.interact",
    );
    expect(matchedRule("rule `fs.write` permits at most risk `low`, but …")).toBe("fs.write");
  });

  it("is not guessed from a reason in any other form", () => {
    expect(matchedRule("sends mail")).toBeNull();
    expect(matchedRule("the rule `x` matched")).toBeNull();
    expect(matchedRule("rule `` matched")).toBeNull();
  });

  it("says what the policy would have done without the taint", () => {
    const tainted = { tainted: true, effect_before_taint: "allow" };
    expect(ruleEffect(approval("a", tainted))).toMatch(/^allows this; the untrusted read/);
    expect(ruleEffect(approval("a", { effect_before_taint: "ask" }))).toBe(
      "asks a person for this",
    );
    expect(ruleEffect(approval("a", { effect_before_taint: "something new" }))).toBeNull();
  });
});

describe("where untrusted data came from", () => {
  it("splits a source label into its kind and the rest, exactly", () => {
    expect(taintOrigin("web:http://127.0.0.1:8420/customers/globex")).toEqual({
      kind: "web",
      detail: "http://127.0.0.1:8420/customers/globex",
      icon: "globe",
    });
    expect(taintOrigin("file:/work/notes.md")).toMatchObject({ kind: "file", icon: "folder" });
    expect(taintOrigin("github:acct@/repos/x")).toEqual({
      kind: "github",
      detail: "acct@/repos/x",
      icon: "link",
    });
  });

  it("draws a source in any other shape whole", () => {
    expect(taintOrigin("user")).toEqual({ kind: null, detail: "user", icon: "link" });
    expect(taintOrigin(" web:x")).toEqual({ kind: null, detail: " web:x", icon: "link" });
    expect(taintOrigin("constructor:x")).toMatchObject({ kind: "constructor", icon: "link" });
  });
});

describe("the mark beside a request", () => {
  it("follows the tool's namespace, and does not guess at one it does not know", () => {
    expect(toolIcon("browser.type")).toBe("window");
    expect(toolIcon("filesystem.write")).toBe("folder");
    expect(toolIcon("network.request")).toBe("globe");
    expect(toolIcon("terminal.exec")).toBe("terminal");
    expect(toolIcon("issues.read")).toBe("zap");
    expect(toolIcon("constructor.x")).toBe("zap");
    expect(toolIcon("")).toBe("zap");
  });
});

describe("the queue item's name", () => {
  it("says who, what, how risky, whether tainted and for how long", () => {
    const now = Date.parse("2026-10-01T10:04:00Z");
    expect(queueItemLabel(approval("a", { risk: "high", tainted: true }), now)).toBe(
      "sales wants to run email.send. high risk, read untrusted data. Blocked for 4m.",
    );
    expect(queueItemLabel(approval("a"), now)).toBe(
      "sales wants to run email.send. low risk. Blocked for 4m.",
    );
  });

  it("says when a decision is half-made on it, or an answer to it failed", () => {
    const now = Date.parse("2026-10-01T10:04:00Z");
    expect(queueItemLabel(approval("a"), now, { armed: true })).toBe(
      "sales wants to run email.send. low risk. Blocked for 4m. A decision is half-made on it.",
    );
    expect(queueItemLabel(approval("a"), now, { failed: true, armed: false })).toBe(
      "sales wants to run email.send. low risk. Blocked for 4m. Your last answer to it did not go through.",
    );
  });
});

describe("the recently decided tally", () => {
  it("counts each way and leaves out what did not happen", () => {
    const tally = recentTally([
      { status: "approved" },
      { status: "denied" },
      { status: "denied" },
      { status: "expired" },
    ]);
    expect(tally).toEqual({ approved: 1, denied: 2, other: 1 });
    expect(tallyLine(tally)).toBe("1 approved · 2 denied · 1 other");
    expect(tallyLine(recentTally([{ status: "denied" }]))).toBe("1 denied");
    expect(tallyLine(recentTally([]))).toBe("none yet");
  });
});
