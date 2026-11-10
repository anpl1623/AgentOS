import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../../bindings/ApprovalView";
import approvalsCss from "../../styles/screens/approvals.css?raw";
import type { IntentKind } from "./approvalsModel";
import {
  type Departure,
  type Stamp,
  answered,
  collapse,
  depart,
  deskEntries,
  failedLine,
  pendingLine,
  settle,
  settledLine,
  settledTone,
  stampOf,
} from "./decisionModel";

function card(id: string): ApprovalView {
  return {
    id,
    agent_name: "sales",
    delegation_chain: [],
    task_id: "task",
    run_id: "run",
    objective: "follow up",
    tool: "browser.type",
    arguments: "{}",
    risk: "high",
    reason: "types",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: null,
    explanation: "Types.",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: "2026-10-01T10:00:00Z",
    decided_at: null,
    note: null,
  };
}

const drawn = (entries: ReturnType<typeof deskEntries>) =>
  entries.map((entry) =>
    entry.kind === "card" ? entry.approval.id : `ghost:${entry.departure.approval.id}`,
  );

describe("what a decision says", () => {
  const request = card("a");

  it("says what is being done while the answer is on its way", () => {
    expect(pendingLine("approve", request)).toBe("Approving browser.type for sales…");
    expect(pendingLine("deny", request)).toBe("Denying browser.type for sales…");
    expect(pendingLine("stop", request)).toBe(
      "Denying browser.type for sales and stopping the run…",
    );
  });

  it("says what was done, and what became of a run asked to stop", () => {
    expect(settledLine("approve", request, null, undefined)).toBe(
      "Approved browser.type for sales.",
    );
    expect(settledLine("stop", request, null, { kind: "stopped" })).toBe(
      "Denied browser.type for sales. The run was stopped.",
    );
    expect(settledLine("stop", request, null, { kind: "failed", message: "It is gone." })).toBe(
      "Denied browser.type for sales. The run could not be stopped: It is gone.",
    );
  });

  it("says a request had moved on in place of a decision it did not receive", () => {
    expect(settledLine("stop", request, "That had moved on.", { kind: "ended" })).toBe(
      "That had moved on. The run had already ended.",
    );
  });

  it("says nothing was changed when the answer could not be sent", () => {
    expect(failedLine("No runtime.")).toBe(
      "Could not answer: No runtime. The request is back in the list; nothing was changed.",
    );
  });

  it("marks an answer given as done, unless the run it was asked to stop is still going", () => {
    expect(settledTone(undefined)).toBe("done");
    expect(settledTone({ kind: "stopped" })).toBe("done");
    expect(settledTone({ kind: "ended" })).toBe("done");
    expect(settledTone({ kind: "failed", message: "It is gone." })).toBe("partial");
  });
});

describe("departing cards", () => {
  it("are stamped, then fold, then go", () => {
    let list: Departure[] = depart([], card("a"), "approve", 0);
    expect(list.map((each) => each.phase)).toEqual(["stamp"]);
    list = collapse(list, "a");
    expect(list.map((each) => each.phase)).toEqual(["collapse"]);
    expect(settle(list, "a")).toEqual([]);
  });

  it("replace an earlier departure of the same card, and never take a negative place", () => {
    const list = depart(depart([], card("a"), "approve", 2), card("a"), "deny", -1);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({ kind: "deny", index: 0, phase: "stamp" });
  });

  it("do not fold back open once folding", () => {
    const folded = collapse(depart([], card("a"), "deny", 0), "a");
    expect(collapse(folded, "a")).toEqual(folded);
  });
});

describe("the stamp on a departing card", () => {
  const KINDS: readonly IntentKind[] = ["approve", "deny", "stop"];
  const stamped = (list: readonly Departure[]) => list.map((each) => stampOf(each));

  it("says the decision is being made until the runtime has answered", () => {
    expect(stamped(depart([], card("a"), "approve", 0))).toEqual([
      { words: "Approving…", tone: "approved", icon: "check" },
    ]);
    expect(stampOf({ kind: "deny", answer: "sent" }).words).toBe("Denying…");
    expect(stampOf({ kind: "stop", answer: "sent" }).words).toBe("Denying, stopping the run…");
  });

  it("says what was decided once the runtime recorded it", () => {
    const list = answered(depart([], card("a"), "approve", 0), "a", "recorded");
    expect(stamped(list)).toEqual([{ words: "Approved", tone: "approved", icon: "check" }]);
    expect(stampOf({ kind: "deny", answer: "recorded" })).toEqual({
      words: "Denied",
      tone: "denied",
      icon: "x",
    });
    expect(stampOf({ kind: "stop", answer: "recorded" }).words).toBe("Denied, stopping the run");
  });

  it.each(KINDS)("claims no %s decision when the request had moved on", (kind) => {
    const list = answered(depart([], card("a"), kind, 0), "a", "moved-on");
    const [stamp] = stamped(list);
    expect(stamp).toEqual({ words: "Moved on; nothing changed", tone: "moved-on", icon: "info" });
    expect(stamp?.words).not.toMatch(/approv|denied|denying/i);
  });

  it("keeps the first answer heard, and leaves other cards and gone ones alone", () => {
    let list = depart(depart([], card("a"), "approve", 0), card("b"), "deny", 1);
    list = answered(list, "a", "moved-on");
    expect(answered(list, "a", "recorded")).toEqual(list);
    expect(list.map((each) => each.answer)).toEqual(["moved-on", "sent"]);
    expect(answered(list, "gone", "recorded")).toEqual(list);
    // A card folding when its answer arrives is still stamped with it.
    const folding = answered(collapse(depart([], card("c"), "approve", 0), "c"), "c", "recorded");
    expect(folding[0]).toMatchObject({ phase: "collapse", answer: "recorded" });
  });

  const TONES: readonly Stamp["tone"][] = ["approved", "denied", "moved-on"];

  it.each(TONES)("is drawn in the %s tone by the stylesheet", (tone) => {
    // Every tone `stampOf` names has its card, its mark and its queue item.
    expect(approvalsCss).toContain(`.approval.departing.${tone} {`);
    expect(approvalsCss).toContain(`.approval.${tone} .stamp-mark {`);
    expect(approvalsCss).toContain(`.approval.${tone} .approval-foot.stamped {`);
    expect(approvalsCss).toContain(`.rail-ghost.${tone} .queue-item {`);
    expect(approvalsCss).toContain(`.rail-ghost.${tone} .queue-item-stamp {`);
  });
});

describe("the drawn queue", () => {
  it("puts a departing card back where it stood", () => {
    const entries = deskEntries([card("a"), card("c")], depart([], card("b"), "approve", 1));
    expect(drawn(entries)).toEqual(["a", "ghost:b", "c"]);
  });

  it("keeps two cards answered in a row in the order they stood", () => {
    // a was answered from the top; b, now at the top, was answered next.
    let leaving = depart([], card("a"), "approve", 0);
    leaving = depart(leaving, card("b"), "deny", 0);
    expect(drawn(deskEntries([card("c")], leaving))).toEqual(["ghost:a", "ghost:b", "c"]);
  });

  it("puts a card that stood past the end at the end", () => {
    expect(drawn(deskEntries([], depart([], card("z"), "approve", 5)))).toEqual(["ghost:z"]);
  });

  it("never shows a departing card beside the real one restored after a failed answer", () => {
    const entries = deskEntries([card("a"), card("b")], depart([], card("a"), "approve", 0));
    expect(drawn(entries)).toEqual(["a", "b"]);
  });

  it("numbers the waiting cards by their place in the queue, not on screen", () => {
    const entries = deskEntries([card("a"), card("c")], depart([], card("b"), "approve", 1));
    expect(entries.flatMap((entry) => (entry.kind === "card" ? [entry.index] : []))).toEqual([
      0, 1,
    ]);
  });
});
