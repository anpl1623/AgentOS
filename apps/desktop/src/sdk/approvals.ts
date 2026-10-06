/**
 * The pending-approval queue: one copy of it, for the badge and the screen.
 *
 * The queue learns from three sources, in decreasing order of freshness. The
 * requested event carries the whole card, so it is inserted as delivered. The
 * operator's own click hides the card before the call is even sent. The
 * resolved event says a request is no longer waiting. Reading the pending list
 * from the runtime is the slow safety net that corrects anything the other
 * three missed.
 *
 * The safety net cannot simply be trusted, because on one path it is wrong
 * every time. `DesktopApprovalGate::request` emits the resolved event and
 * returns, and only then does `RunApprovalGate::request` record the decision,
 * so a read issued in that window still lists the request as pending. Moving
 * the write earlier would record a decision that had not yet been delivered,
 * which is worse; so the window has to know not to believe that read.
 *
 * It must equally not disbelieve the runtime for long. A card hidden while its
 * run is still blocked is the worst failure this store can have — the agent
 * waits forever for an answer nobody is being asked for. The exact rule is on
 * {@link reconciled}, and the tests in `approvals.test.ts` hold it.
 *
 * The rules are pure functions over {@link ApprovalQueue}, with no React and no
 * runtime, so every transition can be tested directly. The hooks below them
 * are thin wrappers.
 */

import { useEffect, useSyncExternalStore } from "react";

import type { ApprovalDecisionInput } from "../bindings/ApprovalDecisionInput";
import type { ApprovalView } from "../bindings/ApprovalView";
import { raise } from "./alerts";
import { api, describeError, events } from "./client";
import { instant } from "./eventStream";
import { listenLive, startVisibleInterval } from "./live";

/**
 * How long after a request settles a pending read is still assumed stale.
 *
 * Covers the gap between the bridge delivering an answer and the run writing
 * it down, which is one database write. Two seconds is generous for that and
 * short against the cost of the alternative: a pending read issued at least
 * this long after a request settled is believed.
 */
export const SETTLE_GRACE_MS = 2000;

/** How often the queue is checked against the runtime while visible. */
export const RECONCILE_EVERY_MS = 15_000;

/** Why a card is hidden. */
export interface Suppression {
  /** When it was hidden. */
  since: number;
  /** When the runtime answered, or `null` while the call is still in flight. */
  settledAt: number | null;
  /** The runtime itself said the request is no longer waiting. */
  confirmed: boolean;
  /**
   * The answer came back undelivered: the approval bridge, which holds every
   * run in this process that is waiting on a human, had nothing waiting on
   * this request. Nothing can ever answer it, so no later read restores it.
   * Such rows are left behind by a process that stopped with a card open, and
   * the runtime lists them for as long as the row exists.
   */
  unanswerable: boolean;
}

/** The queue, and everything needed to decide what it shows. */
export interface ApprovalQueue {
  /** Every request believed pending, hidden or not, oldest first. */
  items: readonly ApprovalView[];
  /** When each request that arrived by event was received. */
  receivedAt: ReadonlyMap<string, number>;
  /** Requests hidden although the runtime may still list them. */
  suppressed: ReadonlyMap<string, Suppression>;
  /** Why the last attempt to answer a request failed, by request. */
  failures: ReadonlyMap<string, string>;
  /** When the most recently applied read of the pending list was issued. */
  lastReconciled: number;
  /** The pending list has been read at least once. */
  loaded: boolean;
}

/** A queue that knows nothing yet. */
export function emptyQueue(): ApprovalQueue {
  return {
    items: [],
    receivedAt: new Map(),
    suppressed: new Map(),
    failures: new Map(),
    lastReconciled: Number.NEGATIVE_INFINITY,
    loaded: false,
  };
}

function byRequestedAt(a: ApprovalView, b: ApprovalView): number {
  return (
    instant(a.requested_at) - instant(b.requested_at) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
  );
}

function without<V>(map: ReadonlyMap<string, V>, id: string): Map<string, V> {
  const next = new Map(map);
  next.delete(id);
  return next;
}

/** The cards to show: every pending request not hidden, oldest first. */
export function visibleApprovals(queue: ApprovalQueue): ApprovalView[] {
  return queue.items.filter((item) => !queue.suppressed.has(item.id));
}

/**
 * A request event arrived. Its payload is the whole card, so it is inserted
 * directly rather than paying for a round trip to learn what was just said.
 */
export function requested(queue: ApprovalQueue, view: ApprovalView, now: number): ApprovalQueue {
  const items = [...queue.items.filter((item) => item.id !== view.id), view].sort(byRequestedAt);
  return { ...queue, items, receivedAt: new Map(queue.receivedAt).set(view.id, now) };
}

/**
 * The operator answered a card; hide it before the call is sent, so the card
 * leaves and the badge drops on the click rather than on the round trip.
 */
export function resolving(queue: ApprovalQueue, id: string, now: number): ApprovalQueue {
  const suppressed = new Map(queue.suppressed);
  if (!suppressed.has(id)) {
    suppressed.set(id, { since: now, settledAt: null, confirmed: false, unanswerable: false });
  }
  return { ...queue, suppressed, failures: without(queue.failures, id) };
}

/**
 * The runtime answered the call, delivered or not. The card stays hidden
 * either way. Delivered, it is done, and the grace period runs from here: a
 * read issued after it that still lists the request is believed, because the
 * run may still be waiting. Not delivered, nothing was waiting for it — the
 * run finished, was cancelled, or belonged to a process that has gone — and
 * the card stays hidden for good: restoring it would offer a choice that does
 * not exist, and clicking it again would only repeat the round trip.
 */
export function resolveSettled(
  queue: ApprovalQueue,
  id: string,
  now: number,
  delivered = true,
): ApprovalQueue {
  const current = queue.suppressed.get(id);
  if (current === undefined || current.settledAt !== null) return queue;
  const suppressed = new Map(queue.suppressed).set(id, {
    ...current,
    settledAt: now,
    unanswerable: !delivered,
  });
  return { ...queue, suppressed };
}

/**
 * The call failed. The card comes back with the reason, because nothing was
 * delivered and the run is still waiting — unless the runtime has meanwhile
 * said the request is resolved, in which case there is nothing to come back to.
 */
export function resolveFailed(queue: ApprovalQueue, id: string, message: string): ApprovalQueue {
  const current = queue.suppressed.get(id);
  if (current?.confirmed === true) return queue;
  const present = queue.items.some((item) => item.id === id);
  return {
    ...queue,
    suppressed: without(queue.suppressed, id),
    failures: present ? new Map(queue.failures).set(id, message) : queue.failures,
  };
}

/**
 * A resolved event arrived: the request is no longer waiting, whoever answered
 * it. It is removed and suppressed, so a pending read issued before the
 * decision was written cannot put it back.
 */
export function resolvedElsewhere(queue: ApprovalQueue, id: string, now: number): ApprovalQueue {
  const current = queue.suppressed.get(id);
  const suppressed = new Map(queue.suppressed).set(id, {
    since: current?.since ?? now,
    settledAt: current?.settledAt ?? now,
    confirmed: true,
    unanswerable: current?.unanswerable ?? false,
  });
  return {
    ...queue,
    items: queue.items.filter((item) => item.id !== id),
    receivedAt: without(queue.receivedAt, id),
    suppressed,
    failures: without(queue.failures, id),
  };
}

/**
 * Apply a read of the pending list that was issued at `issuedAt`.
 *
 * The rule, in full:
 *
 * - A read issued before the last one applied is ignored; it describes an
 *   older world than the one already on screen.
 * - The list becomes the runtime's, plus any request that arrived by event at
 *   or after `issuedAt`. The read could not have seen those, and dropping one
 *   would hide a run that has just started waiting.
 * - A hidden request the read does not list is forgotten, provided the read
 *   was issued after it was hidden. That is what stops the suppressed set
 *   growing without bound, and the proviso stops a read that simply predates a
 *   request from being taken as proof that it has gone.
 * - A hidden request the read does list stays hidden while the call is in
 *   flight, and while the read was issued within {@link SETTLE_GRACE_MS} of
 *   the request settling — the window in which the runtime's own ordering
 *   makes it report a decided request as pending. One the bridge said nothing
 *   was waiting on stays hidden however long it is listed.
 * - Otherwise the read is believed and the card comes back. A request still
 *   listed by a read issued after the grace period is one the runtime is still
 *   waiting on, whatever this window thought it had done.
 */
export function reconciled(
  queue: ApprovalQueue,
  server: readonly ApprovalView[],
  issuedAt: number,
): ApprovalQueue {
  if (issuedAt < queue.lastReconciled) return queue;

  const listed = new Set(server.map((item) => item.id));
  const arrivedSince = queue.items.filter(
    (item) =>
      !listed.has(item.id) &&
      (queue.receivedAt.get(item.id) ?? Number.NEGATIVE_INFINITY) >= issuedAt,
  );
  const items = [...server, ...arrivedSince].sort(byRequestedAt);
  const present = new Set(items.map((item) => item.id));

  const suppressed = new Map<string, Suppression>();
  for (const [id, hold] of queue.suppressed) {
    if (!listed.has(id)) {
      if (issuedAt >= hold.since && !present.has(id)) continue;
      suppressed.set(id, hold);
      continue;
    }
    const withinGrace = hold.settledAt === null || issuedAt < hold.settledAt + SETTLE_GRACE_MS;
    if (hold.unanswerable || withinGrace) suppressed.set(id, hold);
  }

  const receivedAt = new Map<string, number>();
  for (const item of arrivedSince) {
    const at = queue.receivedAt.get(item.id);
    if (at !== undefined) receivedAt.set(item.id, at);
  }
  const failures = new Map([...queue.failures].filter(([id]) => present.has(id)));

  return { items, receivedAt, suppressed, failures, lastReconciled: issuedAt, loaded: true };
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/** The queue as a screen reads it. */
export interface PendingApprovals {
  /** The cards to show, oldest first. */
  approvals: readonly ApprovalView[];
  /** The pending list has been read at least once. */
  loaded: boolean;
  /** Why the last read of the pending list failed, if it did. */
  error: string | null;
  /** Why the last answer to a card failed, by request, for the restored card. */
  failures: ReadonlyMap<string, string>;
}

/** What became of an answer. */
export type ResolveOutcome =
  | { kind: "delivered" }
  | { kind: "moved-on" }
  | { kind: "failed"; message: string };

/** The note shown when an answer arrived after its run had moved on. */
export const MOVED_ON = "That request had already moved on; nothing was changed.";

let queue = emptyQueue();
let readError: { issuedAt: number; message: string } | null = null;
let snapshot: PendingApprovals = derive();
const listeners = new Set<() => void>();
let holders = 0;
let stopSources: (() => void) | null = null;
let verifyTimer: ReturnType<typeof setTimeout> | null = null;

function clock(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function derive(): PendingApprovals {
  return {
    approvals: visibleApprovals(queue),
    loaded: queue.loaded,
    error: readError?.message ?? null,
    failures: queue.failures,
  };
}

function commit(next: ApprovalQueue): void {
  queue = next;
  snapshot = derive();
  for (const listener of listeners) listener();
}

/**
 * Check the hidden cards once their grace period has passed, rather than
 * leaving a wrongly hidden card to wait for the next scheduled read.
 */
function scheduleVerification(): void {
  if (verifyTimer !== null) clearTimeout(verifyTimer);
  verifyTimer = setTimeout(() => {
    verifyTimer = null;
    void refreshApprovals();
  }, SETTLE_GRACE_MS + 250);
}

/** Read the pending list from the runtime and reconcile the queue with it. */
export async function refreshApprovals(): Promise<void> {
  const issuedAt = clock();
  try {
    const server = await api.listPendingApprovals();
    if (readError !== null && readError.issuedAt <= issuedAt) readError = null;
    commit(reconciled(queue, server, issuedAt));
  } catch (failure) {
    if (issuedAt >= queue.lastReconciled) {
      readError = { issuedAt, message: describeError(failure) };
      commit(queue);
    }
  }
}

/**
 * Answer a request.
 *
 * Never rejects; the outcome says what happened. A failure restores the card
 * with the reason on it. An answer that found nothing waiting is reported as a
 * quiet note, not an error: the run finished or was cancelled while the card
 * was on screen, and nothing the operator did went wrong.
 */
export async function resolveApproval(input: ApprovalDecisionInput): Promise<ResolveOutcome> {
  const id = input.approval_id;
  commit(resolving(queue, id, clock()));
  try {
    const delivered = await api.resolveApproval(input);
    commit(resolveSettled(queue, id, clock(), delivered));
    if (delivered) {
      scheduleVerification();
      return { kind: "delivered" };
    }
    raise({ id: `approval-moved-on:${id}`, level: "info", message: MOVED_ON });
    return { kind: "moved-on" };
  } catch (failure) {
    const message = describeError(failure);
    commit(resolveFailed(queue, id, message));
    if (!snapshot.approvals.some((item) => item.id === id)) {
      // The card could not be restored, so the reason would otherwise go unseen.
      raise({ id: `approval-failed:${id}`, level: "error", message });
    }
    return { kind: "failed", message };
  }
}

function start(): () => void {
  const stopRequested = listenLive<ApprovalView>(events.approvalRequested, (view) =>
    commit(requested(queue, view, clock())),
  );
  const stopResolved = listenLive<string>(events.approvalResolved, (id) => {
    commit(resolvedElsewhere(queue, id, clock()));
    scheduleVerification();
  });
  const stopInterval = startVisibleInterval(() => void refreshApprovals(), RECONCILE_EVERY_MS);
  void refreshApprovals();
  return () => {
    stopRequested();
    stopResolved();
    stopInterval();
  };
}

function subscribeQueue(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * The pending queue, shared by every component that reads it.
 *
 * The first component to mount starts the listeners and the reconciling poll;
 * the last to unmount stops them. The queue itself persists in between.
 */
export function usePendingApprovals(): PendingApprovals {
  useEffect(() => {
    holders += 1;
    if (holders === 1) stopSources = start();
    return () => {
      holders -= 1;
      if (holders === 0) {
        stopSources?.();
        stopSources = null;
      }
    };
  }, []);
  return useSyncExternalStore(subscribeQueue, () => snapshot);
}
