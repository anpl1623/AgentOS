import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../bindings/ApprovalView";
import {
  type Selection,
  type SelectionAction,
  arrivals,
  askedOnlyForTaint,
  blockedFor,
  budgetLine,
  decisionTone,
  describedWarning,
  idleSelection,
  isDeliberate,
  nextSelection,
  offersStop,
  sortApprovals,
} from "./approvalsModel";

function approval(
  id: string,
  risk: string,
  requestedAt: string,
  extra: Partial<ApprovalView> = {},
) {
  const view: ApprovalView = {
    id,
    agent_name: "sales",
    task_id: "task",
    run_id: "run",
    objective: "follow up",
    tool: "email.send",
    arguments: "{}",
    risk,
    reason: "sends mail",
    effect_before_taint: "ask",
    asked_this_run: 1,
    approval_budget: 10,
    explanation: "Sends one email.",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: requestedAt,
    decided_at: null,
    note: null,
    ...extra,
  };
  return view;
}

function run(actions: readonly SelectionAction[], from: Selection = idleSelection): Selection {
  return actions.reduce(nextSelection, from);
}

describe("ordering the queue", () => {
  it("puts the most dangerous first, and the longest waiting first within a level", () => {
    const sorted = sortApprovals([
      approval("low-old", "low", "2026-10-01T10:00:00Z"),
      approval("critical-new", "critical", "2026-10-01T10:05:00Z"),
      approval("high-new", "high", "2026-10-01T10:04:00Z"),
      approval("high-old", "high", "2026-10-01T10:01:00Z"),
      approval("none", "none", "2026-09-30T10:00:00Z"),
      approval("medium", "medium", "2026-10-01T10:02:00Z"),
    ]);
    expect(sorted.map((each) => each.id)).toEqual([
      "critical-new",
      "high-old",
      "high-new",
      "medium",
      "low-old",
      "none",
    ]);
  });

  it("puts a risk this build does not know above critical", () => {
    const sorted = sortApprovals([
      approval("critical", "critical", "2026-10-01T10:00:00Z"),
      approval("unknown", "catastrophic", "2026-10-01T10:05:00Z"),
    ]);
    expect(sorted.map((each) => each.id)).toEqual(["unknown", "critical"]);
  });

  it("does not let an unreadable time jump its level", () => {
    const sorted = sortApprovals([
      approval("garbled", "high", "yesterday"),
      approval("dated", "high", "2026-10-01T10:00:00Z"),
    ]);
    expect(sorted.map((each) => each.id)).toEqual(["dated", "garbled"]);
  });

  it("does not reorder the list it was given", () => {
    const list = [
      approval("a", "low", "2026-10-01T10:00:00Z"),
      approval("b", "high", "2026-10-01T10:00:00Z"),
    ];
    sortApprovals(list);
    expect(list.map((each) => each.id)).toEqual(["a", "b"]);
  });
});

describe("blocked for", () => {
  const asked = "2026-10-01T10:00:00Z";
  const at = (ms: number) => Date.parse(asked) + ms;

  it("reads to the minute, rounded down", () => {
    expect(blockedFor(asked, at(0))).toBe("under a minute");
    expect(blockedFor(asked, at(59_999))).toBe("under a minute");
    expect(blockedFor(asked, at(4 * 60_000 + 59_000))).toBe("4m");
    expect(blockedFor(asked, at(60 * 60_000))).toBe("1h");
    expect(blockedFor(asked, at(125 * 60_000))).toBe("2h 5m");
    expect(blockedFor(asked, at(26 * 3_600_000))).toBe("1d 2h");
    expect(blockedFor(asked, at(48 * 3_600_000))).toBe("2d");
  });

  it("says nothing false about a clock that runs behind the request or a bad time", () => {
    expect(blockedFor(asked, at(-30_000))).toBe("under a minute");
    expect(blockedFor("not a time", at(0))).toBe("an unknown time");
  });
});

describe("what the card says about why it is asking", () => {
  it("counts against the budget, and says nothing without one", () => {
    expect(budgetLine(3, 10)).toBe("asked 3 of 10 times this run");
    expect(budgetLine(3, null)).toBeNull();
  });

  it("names taint as the only reason only when the policy would have allowed it", () => {
    const base = approval("a", "high", "2026-10-01T10:00:00Z");
    expect(askedOnlyForTaint({ ...base, tainted: true, effect_before_taint: "allow" })).toBe(true);
    expect(askedOnlyForTaint({ ...base, tainted: true, effect_before_taint: "ask" })).toBe(false);
    expect(askedOnlyForTaint({ ...base, tainted: false, effect_before_taint: "allow" })).toBe(
      false,
    );
  });

  it("offers to stop the run when it is tainted or the risk is high", () => {
    const at = "2026-10-01T10:00:00Z";
    expect(offersStop(approval("a", "low", at))).toBe(false);
    expect(offersStop(approval("a", "medium", at))).toBe(false);
    expect(offersStop(approval("a", "low", at, { tainted: true }))).toBe(true);
    expect(offersStop(approval("a", "high", at))).toBe(true);
    expect(offersStop(approval("a", "critical", at))).toBe(true);
    expect(offersStop(approval("a", "catastrophic", at))).toBe(true);
  });

  it("tones a decision by its status", () => {
    expect(decisionTone("approved")).toBe("ok");
    expect(decisionTone("denied")).toBe("blocked");
    expect(decisionTone("expired")).toBe("neutral");
    expect(decisionTone("constructor")).toBe("neutral");
  });
});

describe("deliberate clicks", () => {
  it("accepts a single click and a keyboard activation, not the rest of a multi-click", () => {
    expect(isDeliberate(0)).toBe(true);
    expect(isDeliberate(1)).toBe(true);
    expect(isDeliberate(2)).toBe(false);
    expect(isDeliberate(3)).toBe(false);
  });
});

describe("selection", () => {
  const order = ["a", "b", "c"];

  it("starts at the first card going down and the last going up, and stops at the ends", () => {
    expect(run([{ kind: "move", by: 1, order }]).selected).toBe("a");
    expect(run([{ kind: "move", by: -1, order }]).selected).toBe("c");
    expect(
      run([
        { kind: "move", by: 1, order },
        { kind: "move", by: 1, order },
        { kind: "move", by: 1, order },
        { kind: "move", by: 1, order },
      ]).selected,
    ).toBe("c");
    expect(run([{ kind: "move", by: -1, order: [] }]).selected).toBeNull();
  });

  it("moves from wherever the selected card now is, not from where it was", () => {
    const state = run([{ kind: "select", id: "b" }]);
    // A critical card arrived above b; moving down still means the card after b.
    expect(
      nextSelection(state, { kind: "move", by: 1, order: ["z", "a", "b", "c"] }).selected,
    ).toBe("c");
  });

  it("keeps an intent on its card while the selection moves", () => {
    const state = run([
      { kind: "intend", id: "a", intent: "approve" },
      { kind: "move", by: 1, order },
      { kind: "move", by: 1, order },
    ]);
    expect(state.selected).toBe("c");
    expect(state.intent).toEqual({ id: "a", kind: "approve", note: "" });
  });
});

describe("intent", () => {
  it("is bound to a card id, and arming another card disarms the first", () => {
    const state = run([
      { kind: "intend", id: "a", intent: "deny" },
      { kind: "note", id: "a", text: "not mine" },
      { kind: "intend", id: "b", intent: "approve" },
    ]);
    expect(state.intent).toEqual({ id: "b", kind: "approve", note: "" });
    expect(state.selected).toBe("b");
  });

  it("takes a note only for the card that holds the intent", () => {
    const state = run([
      { kind: "intend", id: "a", intent: "stop" },
      { kind: "note", id: "b", text: "stray" },
      { kind: "note", id: "a", text: "it read the page" },
    ]);
    expect(state.intent).toEqual({ id: "a", kind: "stop", note: "it read the page" });
  });

  it("is cleared by cancel, which leaves the selection where it was", () => {
    const state = run([{ kind: "intend", id: "a", intent: "deny" }, { kind: "cancel" }]);
    expect(state).toEqual({ selected: "a", intent: null });
  });

  it("after a resolution, nothing is selected and no intent is set", () => {
    const state = run([
      { kind: "move", by: 1, order: ["a", "b"] },
      { kind: "intend", id: "a", intent: "approve" },
      { kind: "resolved" },
      // The decided card leaves and the next one takes its place.
      { kind: "queue", ids: ["b"] },
    ]);
    expect(state).toEqual(idleSelection);
  });

  it("resets on a resolution even while the decided card is still in the queue", () => {
    // A failed answer puts the card back. It must come back unarmed, not one
    // press from the approval the operator thought had been sent.
    const armed: Selection = { selected: "a", intent: { id: "a", kind: "approve", note: "x" } };
    expect(nextSelection(armed, { kind: "resolved" })).toEqual(idleSelection);
  });

  it("drops an intent for a card that left the queue, and keeps one that did not", () => {
    const armed = run([{ kind: "intend", id: "a", intent: "approve" }]);
    expect(nextSelection(armed, { kind: "queue", ids: ["b", "c"] })).toEqual(idleSelection);
    expect(nextSelection(armed, { kind: "queue", ids: ["z", "a"] })).toBe(armed);
  });

  it("does not pass to a card that later takes the same place in the list", () => {
    const armed = run([{ kind: "intend", id: "a", intent: "approve" }]);
    const after = run(
      [
        { kind: "queue", ids: ["b"] },
        { kind: "queue", ids: ["b", "a2"] },
      ],
      armed,
    );
    expect(after.intent).toBeNull();
  });
});

describe("announcing arrivals", () => {
  const first = approval("a", "low", "2026-10-01T10:00:00Z");
  const second = approval("b", "high", "2026-10-01T10:01:00Z", {
    agent_name: "ops",
    tool: "browser.type",
  });
  const third = approval("c", "low", "2026-10-01T10:02:00Z");

  it("announces nothing for the queue as it first loads", () => {
    const loaded = arrivals(null, [first, second]);
    expect(loaded.notice).toBeNull();
    expect([...loaded.seen].sort()).toEqual(["a", "b"]);
  });

  it("announces one line for a new request, and none for one seen before", () => {
    const loaded = arrivals(null, [first]);
    const grown = arrivals(loaded.seen, [second, first]);
    expect(grown.notice).toBe("New request: ops wants to run browser.type, high risk.");
    // Back after a failed answer: not news.
    expect(arrivals(grown.seen, [second]).notice).toBeNull();
    expect(arrivals(arrivals(grown.seen, []).seen, [first, second]).notice).toBeNull();
  });

  it("counts several arrivals and names the first in reading order", () => {
    const notice = arrivals(new Set(["a"]), sortApprovals([first, third, second])).notice;
    expect(notice).toBe("2 new requests. The first: ops wants to run browser.type, high risk.");
  });
});

describe("the description's hidden characters", () => {
  it("are counted across the summary, the reason, the resources and the sources", () => {
    expect(describedWarning(approval("a", "low", "2026-10-01T10:00:00Z"))).toBeNull();
    const view = approval("a", "low", "2026-10-01T10:00:00Z", {
      explanation: "Run `rm -rf \u202efdp.` in /work",
      reason: "writes \u200b/etc",
      affected_resources: ["/work/a\u2066b"],
      taint_sources: ["https://evil.example/\u{e0080}"],
    });
    expect(describedWarning(view)).toMatch(/^This request's description holds 4 invisible/);
    expect(
      describedWarning(
        approval("a", "low", "2026-10-01T10:00:00Z", { objective: "x\u202ey" }),
      ),
    ).toMatch(/holds 1 invisible or direction-changing character,/);
  });
});
