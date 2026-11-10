/**
 * How the run screen keeps up with a run, apart from the hooks that do it.
 *
 * A run changes in bursts: one model turn writes a step, an execution, a
 * permission decision and often an approval within a few milliseconds. The
 * screen reloads once per burst, and a slow timer behind the event stream
 * catches whatever the stream dropped.
 */

import type { EventView } from "../../../bindings/EventView";

/**
 * How long a burst of events for this run is gathered before one reload.
 *
 * One model turn emits a step, an execution, a permission decision and often
 * an approval within a few milliseconds. Reloading on each would fetch the
 * whole trace four or five times for one change.
 */
export const REFRESH_DEBOUNCE_MS = 150;

/**
 * The longest a burst may hold its reload back.
 *
 * A run that emits an event every hundred milliseconds would otherwise keep a
 * trailing debounce waiting for a quiet moment that never comes, and the trace
 * would stand still while the run moved.
 */
export const REFRESH_MAX_WAIT_MS = 1_000;

/**
 * The safety-net reload while a run is live.
 *
 * The event stream is lossy under lag — the runtime drops what a slow window
 * cannot take — so an event-driven screen still re-reads on a timer. Slowed,
 * never removed.
 */
export const LIVE_INTERVAL_MS = 10_000;

/** The reload once a run has ended; only a retry elsewhere or a late record changes it. */
export const SETTLED_INTERVAL_MS = 60_000;

/**
 * How often a live run's elapsed time and timeline extend to the present when
 * nothing else changes.
 */
export const LIVE_CLOCK_MS = 5_000;

/**
 * Whether a streamed event concerns this run.
 *
 * The payload arrives from the runtime untyped, so its shape is checked
 * rather than assumed.
 */
export function concernsRun(payload: unknown, runId: string): boolean {
  return concernsAnyRun(payload, new Set([runId]));
}

/**
 * Whether a streamed event concerns any of `runIds`.
 *
 * A run that delegated is changed by its children's events as well as its
 * own: a child's step moves the rows beneath the call, and a child's taint is
 * the parent's taint.
 */
export function concernsAnyRun(payload: unknown, runIds: ReadonlySet<string>): boolean {
  if (typeof payload !== "object" || payload === null) return false;
  const runId = (payload as Partial<EventView>).run_id;
  return typeof runId === "string" && runIds.has(runId);
}

/**
 * A trailing debounce: `schedule` as often as you like, `fn` runs once after a
 * quiet `waitMs`. `flush` runs it now, in place of anything pending.
 */
export interface Coalescer {
  schedule: () => void;
  flush: () => void;
  cancel: () => void;
}

/**
 * Coalesce calls into one, `waitMs` after the last of them, and never more
 * than `maxWaitMs` after the first.
 *
 * Trailing rather than leading: the first event of a model turn is the least
 * informative of its burst, and a reload fired on it would miss the rest and
 * need another. Bounded, so that a burst which never pauses still reloads.
 */
export function coalesce(fn: () => void, waitMs: number, maxWaitMs = Infinity): Coalescer {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let since: number | undefined;
  const cancel = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    since = undefined;
  };
  const flush = () => {
    cancel();
    fn();
  };
  return {
    schedule: () => {
      const now = Date.now();
      since ??= now;
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(flush, Math.max(0, Math.min(waitMs, since + maxWaitMs - now)));
    },
    flush,
    cancel,
  };
}
