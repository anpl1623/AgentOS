/**
 * What the feed tells the eye beyond its rows: which rows have just arrived,
 * how many came in while the operator was reading further up, how busy the
 * last hour was, whether the stream is live, and what an empty feed offers
 * to do next. Pure, so each can be held by a test.
 */

import type { EventView } from "../../bindings/EventView";
import { instant } from "../../sdk/eventStream";
import type { LiveTone } from "../../components/LiveDot";

/**
 * More than this many unknown events at once is a read, not an arrival.
 *
 * The stream delivers in small batches; a history read, a deeper load or the
 * switch to the security view brings in hundreds. Animating those would make
 * a filter change look like a flood of news.
 */
export const ARRIVAL_LIMIT = 12;

/**
 * The events that arrived since the window was last seen.
 *
 * `known` is the set of ids the window held at the last commit, or `null`
 * before it first held anything, when nothing has "arrived" yet. An arrival is
 * an event that is not known and sits after the newest known event: events
 * read from further back sort before it, and are history, not news. A window
 * with no known event in it at all has been replaced rather than added to.
 */
export function freshArrivals(
  known: ReadonlySet<string> | null,
  events: readonly Pick<EventView, "id">[],
  limit: number = ARRIVAL_LIMIT,
): Set<string> {
  const fresh = new Set<string>();
  if (known === null || known.size === 0) return fresh;
  let lastKnown = -1;
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event !== undefined && known.has(event.id)) {
      lastKnown = index;
      break;
    }
  }
  if (lastKnown === -1) return fresh;
  for (const event of events.slice(lastKnown + 1)) {
    if (!known.has(event.id)) fresh.add(event.id);
  }
  return fresh.size > limit ? new Set() : fresh;
}

/**
 * How many events follow `anchor` in the list: the ones that came in after
 * Follow was switched off, which the jump button counts.
 *
 * An anchor that is no longer in the list — filtered out, or dropped from the
 * window — gives zero rather than a guess.
 */
export function countAfter(
  events: readonly Pick<EventView, "id">[],
  anchor: string | null,
): number {
  if (anchor === null) return 0;
  const index = events.findIndex((event) => event.id === anchor);
  return index === -1 ? 0 : events.length - 1 - index;
}

/** The jump button's words. */
export function unseenLabel(count: number): string {
  return count === 1 ? "1 new event" : `${count} new events`;
}

/** How many buckets the rate sparkline has, and how wide each one is. */
export const RATE_BUCKETS = 12;
export const RATE_BUCKET_MS = 5 * 60_000;

/**
 * Events per bucket over the window that ends at `now`, oldest first.
 *
 * Twelve five-minute buckets is the last hour. An event stamped a little in
 * the future, as a clock that has drifted can do, counts in the newest bucket
 * rather than vanishing; one with no readable time is left out.
 */
export function rateSeries(
  events: readonly Pick<EventView, "at">[],
  now: number,
  buckets: number = RATE_BUCKETS,
  bucketMs: number = RATE_BUCKET_MS,
): number[] {
  const series = Array.from({ length: buckets }, () => 0);
  const start = now - buckets * bucketMs;
  for (const event of events) {
    const at = instant(event.at);
    if (!Number.isFinite(at) || at < start) continue;
    const bucket = Math.min(buckets - 1, Math.floor((at - start) / bucketMs));
    series[bucket] = (series[bucket] ?? 0) + 1;
  }
  return series;
}

/** The sum of a series. */
export function total(series: readonly number[]): number {
  return series.reduce((sum, value) => sum + value, 0);
}

/** How many of the events are security-relevant. */
export function securityCount(events: readonly Pick<EventView, "security_relevant">[]): number {
  return events.filter((event) => event.security_relevant).length;
}

/** What the Live pill in the header says. */
export interface LiveStatus {
  label: string;
  tone: LiveTone;
  /** The dot pulses only while events are actually flowing. */
  pulse: boolean;
  /** A qualifier beside the label, such as "fixtures", or `null`. */
  note: string | null;
}

/**
 * The state of the shared stream, in a word.
 *
 * Fixture data is still a live stream (the fixtures replay one event every few
 * seconds) but it is said, in the attention hue, because a feed of invented
 * events must never be mistaken for a real one.
 */
export function liveStatus(input: {
  fixtures: boolean;
  loaded: boolean;
  failed: boolean;
}): LiveStatus {
  if (input.failed) return { label: "Not answering", tone: "warn", pulse: false, note: null };
  if (!input.loaded) return { label: "Connecting", tone: "info", pulse: false, note: null };
  if (input.fixtures) return { label: "Live", tone: "warn", pulse: true, note: "fixtures" };
  return { label: "Live", tone: "ok", pulse: true, note: null };
}

/** The one thing an empty feed offers to do about it. */
export type EmptyAction = "retry" | "clear" | "every-run" | "every-event" | "tasks";

/**
 * The next step for an empty feed, for the same reasons `emptyMessage` gives:
 * read again after a failure, drop the filter, widen to every run, leave the
 * security view, or, with nothing recorded at all, go and start some work.
 */
export function emptyAction(input: {
  securityOnly: boolean;
  filtering: boolean;
  runId: string | null;
  failed: boolean;
}): EmptyAction {
  if (input.failed) return "retry";
  if (input.filtering) return "clear";
  if (input.runId !== null) return "every-run";
  if (input.securityOnly) return "every-event";
  return "tasks";
}

/**
 * Whether an element must be scrolled to for the operator to see it: it is
 * not wholly between the top and bottom of the view.
 *
 * An opened record beside the feed is already on screen, and scrolling to it
 * would move the row the operator just pressed out from under them; one
 * opened beneath a long feed is not, and is brought into view.
 */
export function needsReveal(
  box: { top: number; bottom: number },
  view: { top: number; bottom: number },
): boolean {
  return box.top < view.top || box.bottom > view.bottom;
}
