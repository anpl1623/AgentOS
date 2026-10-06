import { describe, expect, it } from "vitest";

import type { ApprovalView } from "../bindings/ApprovalView";
import {
  type ApprovalQueue,
  SETTLE_GRACE_MS,
  emptyQueue,
  reconciled,
  requested,
  resolveFailed,
  resolveSettled,
  resolvedElsewhere,
  resolving,
  visibleApprovals,
} from "./approvals";

function approval(id: string, requestedAt = "2026-09-29T10:00:00Z"): ApprovalView {
  return {
    id,
    agent_name: "sales",
    task_id: "task",
    run_id: "run",
    objective: "follow up",
    tool: "email.send",
    arguments: "{}",
    risk: "high",
    reason: "sends mail",
    explanation: "Sends one email.",
    affected_resources: [],
    tainted: false,
    taint_sources: [],
    status: "pending",
    requested_at: requestedAt,
    decided_at: null,
    note: null,
  };
}

function shown(queue: ApprovalQueue): string[] {
  return visibleApprovals(queue).map((item) => item.id);
}

/** A queue that has read `ids` from the runtime at time 0. */
function loaded(...ids: string[]): ApprovalQueue {
  return reconciled(emptyQueue(), ids.map((id) => approval(id)), 0);
}

describe("inserting from the requested event", () => {
  it("shows the card at once, ordered by when it was requested", () => {
    let queue = emptyQueue();
    queue = requested(queue, approval("late", "2026-09-29T10:00:05Z"), 1);
    queue = requested(queue, approval("early", "2026-09-29T10:00:01Z"), 2);
    expect(shown(queue)).toEqual(["early", "late"]);
  });

  it("replaces a re-delivered request rather than showing it twice", () => {
    let queue = requested(emptyQueue(), approval("a"), 1);
    queue = requested(queue, { ...approval("a"), explanation: "again" }, 2);
    expect(visibleApprovals(queue)).toHaveLength(1);
    expect(visibleApprovals(queue)[0]?.explanation).toBe("again");
  });

  it("orders by the instant, not the text of the stamp", () => {
    let queue = emptyQueue();
    queue = requested(queue, approval("whole", "2026-09-29T10:00:00Z"), 1);
    queue = requested(queue, approval("fraction", "2026-09-29T09:59:59.9+00:00"), 2);
    expect(shown(queue)).toEqual(["fraction", "whole"]);
  });
});

describe("answering a card", () => {
  it("hides the card before the call settles", () => {
    const queue = resolving(loaded("a", "b"), "a", 10);
    expect(shown(queue)).toEqual(["b"]);
  });

  it("restores the card with the reason when the call fails", () => {
    let queue = resolving(loaded("a"), "a", 10);
    queue = resolveFailed(queue, "a", "the runtime is busy");
    expect(shown(queue)).toEqual(["a"]);
    expect(queue.failures.get("a")).toBe("the runtime is busy");
  });

  it("clears the previous failure when the card is answered again", () => {
    let queue = resolveFailed(resolving(loaded("a"), "a", 10), "a", "busy");
    queue = resolving(queue, "a", 20);
    expect(queue.failures.has("a")).toBe(false);
  });

  it("keeps a delivered answer hidden", () => {
    const queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20);
    expect(shown(queue)).toEqual([]);
  });

  it("keeps an answer that found nothing waiting hidden, rather than offering it again", () => {
    // Nothing in this process was waiting on the request, so the card would
    // offer a choice that no longer exists — however long the runtime lists
    // the orphaned row.
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20, false);
    queue = reconciled(queue, [approval("a")], 21);
    expect(shown(queue)).toEqual([]);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS * 10);
    expect(shown(queue)).toEqual([]);
  });

  it("forgets an unanswerable card once the runtime stops listing it", () => {
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20, false);
    queue = reconciled(queue, [], 30);
    expect(queue.suppressed.has("a")).toBe(false);
  });

  it("does not restore a card the runtime has already said is resolved", () => {
    let queue = resolving(loaded("a"), "a", 10);
    queue = resolvedElsewhere(queue, "a", 11);
    queue = resolveFailed(queue, "a", "late failure");
    expect(shown(queue)).toEqual([]);
  });
});

describe("the resolved event", () => {
  it("removes the card", () => {
    const queue = resolvedElsewhere(loaded("a", "b"), "a", 10);
    expect(shown(queue)).toEqual(["b"]);
  });

  it("is not undone by a read issued before it", () => {
    let queue = resolvedElsewhere(loaded("a"), "a", 10);
    queue = reconciled(queue, [approval("a")], 5);
    expect(shown(queue)).toEqual([]);
  });

  it("is not undone by the read that races the decision being written", () => {
    // The runtime emits the event before it records the decision, so a read
    // issued just after the event still lists the request as pending.
    let queue = resolvedElsewhere(loaded("a"), "a", 10);
    queue = reconciled(queue, [approval("a")], 10 + SETTLE_GRACE_MS - 1);
    expect(shown(queue)).toEqual([]);
  });

  it("suppresses a request it has not seen, so an older read cannot introduce it", () => {
    let queue = resolvedElsewhere(emptyQueue(), "ghost", 10);
    queue = reconciled(queue, [approval("ghost")], 9);
    expect(shown(queue)).toEqual([]);
  });
});

describe("reconciling with the runtime", () => {
  it("adopts the runtime's list", () => {
    const queue = reconciled(loaded("a", "b"), [approval("b"), approval("c")], 5);
    expect(shown(queue)).toEqual(["b", "c"]);
    expect(queue.loaded).toBe(true);
  });

  it("ignores a read issued before the one already applied", () => {
    let queue = reconciled(emptyQueue(), [approval("new")], 10);
    queue = reconciled(queue, [approval("old")], 5);
    expect(shown(queue)).toEqual(["new"]);
  });

  it("keeps a request that arrived by event after the read was issued", () => {
    let queue = requested(emptyQueue(), approval("fresh"), 12);
    queue = reconciled(queue, [], 10);
    expect(shown(queue)).toEqual(["fresh"]);
  });

  it("drops a request that arrived by event before the read and is no longer listed", () => {
    let queue = requested(emptyQueue(), approval("gone"), 8);
    queue = reconciled(queue, [], 10);
    expect(shown(queue)).toEqual([]);
  });

  it("forgets hidden requests the runtime no longer lists, so the set stays bounded", () => {
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20);
    queue = reconciled(queue, [], 30);
    expect(queue.suppressed.size).toBe(0);
    expect(queue.items).toHaveLength(0);
  });

  it("does not forget a hidden request because of a read that predates hiding it", () => {
    // The read did not list it only because it had not been requested yet.
    let queue = requested(emptyQueue(), approval("a"), 5);
    queue = resolving(queue, "a", 6);
    queue = reconciled(queue, [], 4);
    expect(queue.suppressed.has("a")).toBe(true);
    queue = reconciled(queue, [approval("a")], 7);
    expect(shown(queue)).toEqual([]);
  });

  it("does not let an older read's silence clear the way for a newer read's stale listing", () => {
    // Read one was issued before the request existed; read two after it was
    // requested but before its decision was written. Both return after the
    // resolved event. Forgetting the suppression on read one would let read
    // two show a card for a request that has already been answered.
    let queue = resolvedElsewhere(emptyQueue(), "a", 10);
    queue = reconciled(queue, [], 8);
    queue = reconciled(queue, [approval("a")], 9);
    expect(shown(queue)).toEqual([]);
  });

  it("keeps a card hidden while its answer is still in flight", () => {
    let queue = resolving(loaded("a"), "a", 10);
    queue = reconciled(queue, [approval("a")], 10 + SETTLE_GRACE_MS * 10);
    expect(shown(queue)).toEqual([]);
  });

  it("drops a card the runtime stops listing while its answer is in flight", () => {
    let queue = resolving(loaded("a"), "a", 10);
    queue = reconciled(queue, [], 11);
    expect(queue.items).toHaveLength(0);
    queue = resolveFailed(queue, "a", "failed");
    expect(shown(queue)).toEqual([]);
    expect(queue.failures.has("a")).toBe(false);
  });

  it("drops failure notes for requests that are gone", () => {
    let queue = resolveFailed(resolving(loaded("a"), "a", 10), "a", "busy");
    queue = reconciled(queue, [], 20);
    expect(queue.failures.size).toBe(0);
  });
});

describe("a card hidden while the run is still waiting", () => {
  // The worst failure this store can have: an agent blocked on an answer
  // nobody is being asked for. Whatever hid the card, a read issued once the
  // grace period after settling has passed that still lists the request wins.

  it("comes back after a delivered answer the runtime still lists", () => {
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS);
    expect(shown(queue)).toEqual(["a"]);
    expect(queue.suppressed.has("a")).toBe(false);
  });

  it("does not come back after an answer that found nothing waiting", () => {
    // Unlike a delivered answer, there is no run for the card to be keeping
    // waiting: the bridge that holds every waiting run had none for it.
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20, false);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS + 1);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS * 5);
    expect(shown(queue)).toEqual([]);
  });

  it("comes back after a resolved event the runtime contradicts", () => {
    let queue = resolvedElsewhere(loaded("a"), "a", 10);
    queue = reconciled(queue, [approval("a")], 10 + SETTLE_GRACE_MS);
    expect(shown(queue)).toEqual(["a"]);
  });

  it("stays visible through later reads once it has come back", () => {
    let queue = resolveSettled(resolving(loaded("a"), "a", 10), "a", 20);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS);
    queue = reconciled(queue, [approval("a")], 20 + SETTLE_GRACE_MS * 2);
    expect(shown(queue)).toEqual(["a"]);
  });

  it("is hidden no longer than the grace period, measured from settling, not clicking", () => {
    // A slow call must not use up the grace period before it has even settled.
    let queue = resolving(loaded("a"), "a", 0);
    queue = resolveSettled(queue, "a", SETTLE_GRACE_MS * 3);
    queue = reconciled(queue, [approval("a")], SETTLE_GRACE_MS * 3 + 1);
    expect(shown(queue)).toEqual([]);
    queue = reconciled(queue, [approval("a")], SETTLE_GRACE_MS * 4);
    expect(shown(queue)).toEqual(["a"]);
  });
});
