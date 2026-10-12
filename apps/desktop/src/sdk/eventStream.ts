/**
 * The activity feed, kept for the life of the window rather than the screen.
 *
 * Started once from the shell, so the feed is continuous: events that arrive
 * while the operator is on another screen are there when they come back,
 * instead of existing only while Activity happens to be mounted.
 *
 * The durable audit log remains the source of truth. What is held here is a
 * view onto it, exactly as the Rust side says of its own stream: a listener
 * that falls behind loses events from the feed and none from the log, and
 * reloading history repairs the view because records merge by id.
 */

import { useSyncExternalStore } from "react";

import type { EventView } from "../bindings/EventView";
import { api, describeError, events as runtimeEvents } from "./client";
import { listenLive } from "./live";

/** How many events the feed holds before dropping the oldest. */
export const EVENT_WINDOW = 2000;

/**
 * How many security-relevant events are held in their own window.
 *
 * Refusals are kept apart from the main window because filtering after a slice
 * lets "refusals only" report nothing while refusals sit just outside it — an
 * all-clear the product must never show by accident. Sized to match the main
 * window, so the security window always holds every refusal the main window
 * does, and more whenever ordinary traffic has pushed them out of it.
 */
export const SECURITY_WINDOW = EVENT_WINDOW;

/** How many stored events to read when (re)loading history. */
export const HISTORY_LOAD = 500;

/** How long streamed events wait so a burst is merged and sorted once. */
const FLUSH_MS = 50;

/** The two windows the feed is drawn from, each oldest first. */
export interface EventWindows {
  all: readonly EventView[];
  security: readonly EventView[];
}

/**
 * A timestamp as milliseconds, keeping precision below a millisecond.
 *
 * The runtime writes RFC 3339 with as many fractional digits as it has, and
 * `Date.parse` is not required to accept more than three. The fraction is
 * therefore split off and added back, so two events a few microseconds apart
 * still sort in the order they happened. An unparseable stamp sorts last
 * rather than poisoning the comparison with `NaN`.
 */
export function instant(at: string): number {
  const match = /^(.*T\d{2}:\d{2}:\d{2})(\.\d+)?(.*)$/.exec(at);
  const whole = match ? Date.parse(`${match[1]}${match[3]}`) : Date.parse(at);
  if (Number.isNaN(whole)) return Number.POSITIVE_INFINITY;
  const fraction = match?.[2] ? Number(match[2]) * 1000 : 0;
  return whole + fraction;
}

/** Parsed stamps, remembered per event so a sort does not re-parse on every comparison. */
const instants = new WeakMap<EventView, number>();

function when(event: EventView): number {
  let value = instants.get(event);
  if (value === undefined) {
    value = instant(event.at);
    instants.set(event, value);
  }
  return value;
}

function byInstant(a: EventView, b: EventView): number {
  return when(a) - when(b) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/**
 * Put events in the order they happened.
 *
 * Stored events carry their position in the audit chain and are ordered by
 * it, because the chain, not the clock, is the record of what came first.
 * Streamed events carry no sequence and are ordered by time. The two runs are
 * then merged by time. A single comparator mixing the rules — sequence when
 * both have one, time otherwise — is not transitive, and a sort given such a
 * comparator is free to return any order at all; treating a missing sequence
 * as zero would be worse still, filing every live event before all of history.
 */
export function orderEvents(list: readonly EventView[]): EventView[] {
  const stored = list
    .filter((event) => event.sequence !== null)
    .sort((a, b) => (a.sequence ?? 0) - (b.sequence ?? 0) || byInstant(a, b));
  const streamed = list.filter((event) => event.sequence === null).sort(byInstant);

  const ordered: EventView[] = [];
  let s = 0;
  let l = 0;
  while (s < stored.length || l < streamed.length) {
    const nextStored = stored[s];
    const nextStreamed = streamed[l];
    if (
      nextStored !== undefined &&
      (nextStreamed === undefined || when(nextStored) <= when(nextStreamed))
    ) {
      ordered.push(nextStored);
      s += 1;
    } else if (nextStreamed !== undefined) {
      ordered.push(nextStreamed);
      l += 1;
    }
  }
  return ordered;
}

/**
 * Merge incoming events into a window, by id, keeping the newest `limit`.
 *
 * When the same event is known twice the stored copy wins, since it carries
 * the sequence the streamed copy lacks.
 */
function mergeInto(
  current: readonly EventView[],
  incoming: readonly EventView[],
  limit: number,
): EventView[] {
  const byId = new Map(current.map((event) => [event.id, event]));
  for (const event of incoming) {
    const existing = byId.get(event.id);
    const keepExisting =
      existing !== undefined && existing.sequence !== null && event.sequence === null;
    if (!keepExisting) byId.set(event.id, event);
  }
  return orderEvents([...byId.values()]).slice(-limit);
}

/** Merge incoming events into both windows. */
export function mergeEvents(
  windows: EventWindows,
  incoming: readonly EventView[],
  limits: { all: number; security: number } = { all: EVENT_WINDOW, security: SECURITY_WINDOW },
): EventWindows {
  return {
    all: mergeInto(windows.all, incoming, limits.all),
    security: mergeInto(
      windows.security,
      incoming.filter((event) => event.security_relevant),
      limits.security,
    ),
  };
}

/** How the operator has chosen to look at the feed. */
export interface FeedFilter {
  /** Show only refusals, escalations and rejections. */
  securityOnly: boolean;
  /** Keep the newest event in view as events arrive. */
  follow: boolean;
}

/** The events a filter shows, oldest first. */
export function feedEvents(windows: EventWindows, filter: FeedFilter): readonly EventView[] {
  return filter.securityOnly ? windows.security : windows.all;
}

/** The feed as a screen reads it. */
export interface EventStream extends EventWindows {
  /** History has been read at least once. */
  loaded: boolean;
  /** How many records the deepest history read so far asked for; 0 before one has answered. */
  depth: number;
  /** Why the last history read failed, if it did. */
  error: string | null;
}

let stream: EventStream = { all: [], security: [], loaded: false, depth: 0, error: null };
let filter: FeedFilter = { securityOnly: false, follow: true };
let started = false;
let pending: EventView[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
const streamListeners = new Set<() => void>();
const filterListeners = new Set<() => void>();

function publish(next: EventStream): void {
  stream = next;
  for (const listener of streamListeners) listener();
}

function flush(): void {
  flushTimer = null;
  const batch = pending;
  pending = [];
  if (batch.length > 0) publish({ ...stream, ...mergeEvents(stream, batch) });
}

/**
 * Start following the runtime's activity. Calling it again does nothing.
 *
 * The listener is never removed: the feed is meant to span the life of the
 * window, and the shared channel it joins costs one callback per event.
 */
export function startEventStream(): void {
  if (started) return;
  started = true;
  listenLive<EventView>(runtimeEvents.activity, (event) => {
    pending.push(event);
    flushTimer ??= setTimeout(flush, FLUSH_MS);
  });
  void reloadEventHistory();
}

/**
 * Read recent history from the audit log and merge it into the feed.
 *
 * `limit` reads further back than launch does. What it brings in stays in the
 * shared window, so a deeper read outlives the screen that asked for it; a
 * later, shallower reload merges by id and does not shorten it.
 */
export async function reloadEventHistory(limit: number = HISTORY_LOAD): Promise<void> {
  try {
    const history = await api.activity(limit);
    publish({
      ...mergeEvents(stream, history),
      loaded: true,
      depth: Math.max(stream.depth, limit),
      error: null,
    });
  } catch (failure) {
    publish({ ...stream, error: describeError(failure) });
  }
}

function subscribeStream(listener: () => void): () => void {
  streamListeners.add(listener);
  return () => streamListeners.delete(listener);
}

function subscribeFilter(listener: () => void): () => void {
  filterListeners.add(listener);
  return () => filterListeners.delete(listener);
}

/** The feed, shared by every component that reads it. */
export function useEvents(): EventStream {
  return useSyncExternalStore(subscribeStream, () => stream);
}

/** Change how the feed is shown; the choice outlives the screen. */
export function setFeedFilter(patch: Partial<FeedFilter>): void {
  filter = { ...filter, ...patch };
  for (const listener of filterListeners) listener();
}

/** How the feed is currently shown. */
export function useFeedFilter(): FeedFilter {
  return useSyncExternalStore(subscribeFilter, () => filter);
}
