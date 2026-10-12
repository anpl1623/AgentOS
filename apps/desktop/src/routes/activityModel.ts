/**
 * The rules of the activity screen, kept apart from React so each can be held
 * by a test.
 *
 * The screen is a view onto the audit log, and the log is the product's
 * strongest claim. Most of what is here exists so the view cannot say more
 * than it knows: an empty filter must not read as an all-clear when the
 * events it would have matched were never loaded, and a held window must say
 * that it is a window.
 */

import type { EventView } from "../bindings/EventView";
import { EVENT_WINDOW, HISTORY_LOAD, instant, orderEvents } from "../sdk/eventStream";

/**
 * How far from the end of the feed, in pixels, still counts as at the end.
 *
 * Enough to absorb a trackpad's overshoot and a row's worth of growth, small
 * enough that scrolling up to read one earlier event is taken as meant.
 */
export const FOLLOW_THRESHOLD_PX = 40;

/**
 * How long after a wheel turn, touch or key press a scroll is taken as the
 * operator's. Scrolls outside it are the screen's own — following the newest
 * event, restoring a remembered position — and must not switch Follow off.
 */
export const USER_SCROLL_WINDOW_MS = 600;

/** How many records `Load 1000` asks the log for. */
export const DEEP_LOAD = 1000;

/** How many records the screen asks the log for before `Load 1000` is pressed. */
export const SHALLOW_LOAD = HISTORY_LOAD;

/** How many leading characters of a hash are shown; the whole value is in a title. */
export const HASH_PREFIX = 16;

/**
 * Merge sources of events into one window, by id, newest `limit` kept.
 *
 * The same event commonly arrives twice, once streamed and once read back
 * from the log. The stored copy wins, because it carries the sequence that
 * makes it openable; which source happened to answer last must not decide it.
 * Rows are keyed by id, so a duplicate left in would also be a duplicate key.
 */
export function mergeFeed(
  sources: readonly (readonly EventView[])[],
  limit: number = EVENT_WINDOW,
): EventView[] {
  const byId = new Map<string, EventView>();
  for (const source of sources) {
    for (const event of source) {
      const existing = byId.get(event.id);
      if (existing !== undefined && existing.sequence !== null && event.sequence === null) {
        continue;
      }
      byId.set(event.id, event);
    }
  }
  return orderEvents([...byId.values()]).slice(-limit);
}

/** What the operator has narrowed the feed to, beyond the security toggle. */
export interface FeedQuery {
  /** Matched case-insensitively against kind and summary. */
  text: string;
  /** One exact kind, or `""` for every kind. */
  kind: string;
  /** One run, from the address, or `null` for every run. */
  runId: string | null;
}

/** An unnarrowed query. */
export const NO_QUERY: FeedQuery = { text: "", kind: "", runId: null };

/** Whether the text or kind filter is narrowing the feed. The run is reported apart. */
export function isFiltering(query: FeedQuery): boolean {
  return query.text.trim() !== "" || query.kind !== "";
}

/** The events a query keeps, in the order given. */
export function filterFeed(events: readonly EventView[], query: FeedQuery): EventView[] {
  const needle = query.text.trim().toLowerCase();
  return events.filter(
    (event) =>
      (query.runId === null || event.run_id === query.runId) &&
      (query.kind === "" || event.kind === query.kind) &&
      (needle === "" ||
        event.kind.toLowerCase().includes(needle) ||
        event.summary.toLowerCase().includes(needle)),
  );
}

/**
 * The kinds the select offers: every distinct kind in the window, sorted.
 *
 * A chosen kind that has left the window — the oldest events were dropped, or
 * the window changed to security records — stays listed, so the select never
 * shows a different choice from the one that is filtering the feed.
 */
export function kindOptions(events: readonly EventView[], selected: string): string[] {
  const kinds = new Set(events.map((event) => event.kind));
  if (selected !== "") kinds.add(selected);
  return [...kinds].sort();
}

/** The key of a day with no readable timestamp. */
export const UNKNOWN_DAY = "unknown";

/** The local calendar day an event happened on, as `YYYY-MM-DD`. */
export function dayKey(at: string): string {
  const ms = instant(at);
  if (!Number.isFinite(ms)) return UNKNOWN_DAY;
  const date = new Date(ms);
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

/** A run of consecutive events from one day. */
export interface DayGroup {
  /** Unique within one grouping, so it can key a React list. */
  id: string;
  /** The day, as {@link dayKey} writes it. */
  day: string;
  events: EventView[];
}

/**
 * Split an ordered feed wherever the day changes between neighbours.
 *
 * The feed is ordered by the chain, not the clock, so a day can recur after
 * another; each recurrence is its own group with its own separator, which is
 * the honest picture. The id carries the group's first event for that reason:
 * the day alone would not be unique.
 */
export function groupByDay(events: readonly EventView[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const event of events) {
    const day = dayKey(event.at);
    const last = groups.at(-1);
    if (last !== undefined && last.day === day) {
      last.events.push(event);
    } else {
      groups.push({ id: `${day}:${event.id}`, day, events: [event] });
    }
  }
  return groups;
}

/**
 * A separator's words. Absolute always, relative as well when it helps: an
 * audit view that only said "Yesterday" would be wrong the morning after.
 */
export function dayLabel(day: string, now: Date): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) return "Date unknown";
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  const written = date.toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (day === dayKey(now.toISOString())) return `Today · ${written}`;
  if (day === dayKey(yesterday.toISOString())) return `Yesterday · ${written}`;
  return written;
}

/**
 * Whether Follow should be on after a scroll.
 *
 * `distance` is how far the end of the feed sits below the bottom of the
 * viewport; negative when the operator has scrolled past it to the record
 * beneath. Only a scroll the operator made changes anything. Measured both
 * ways, so reading an opened record below the feed is not "at the end" and
 * a new event does not pull the record out from under them.
 */
export function followAfterScroll(follow: boolean, distance: number, byOperator: boolean): boolean {
  if (!byOperator) return follow;
  return Math.abs(distance) <= FOLLOW_THRESHOLD_PX;
}

/**
 * Whether an event can be opened as a record.
 *
 * A streamed event has no sequence until it is read back from the log, and
 * the log cannot be asked for a record it has not finished writing.
 */
export function canOpen(event: EventView): boolean {
  return event.sequence !== null;
}

/** The events as JSON Lines, one object per line, ending in a newline. */
export function toJsonl(events: readonly EventView[]): string {
  return events.map((event) => `${JSON.stringify(event)}\n`).join("");
}

/** The first {@link HASH_PREFIX} characters of a hash, marked as cut when they are. */
export function shortHash(hash: string): string {
  return hash.length > HASH_PREFIX ? `${hash.slice(0, HASH_PREFIX)}…` : hash;
}

/**
 * The newest sequence in a window, or `null` when nothing in it is stored.
 *
 * Sequences start at 1 and leave no gaps — the verifier rejects a chain that
 * does — so the newest is also how many records the log held when it was
 * read. That is what lets the screen say how much of the log it is holding
 * without asking for a count.
 */
export function latestSequence(events: readonly EventView[]): number | null {
  let latest: number | null = null;
  for (const event of events) {
    if (event.sequence !== null && (latest === null || event.sequence > latest)) {
      latest = event.sequence;
    }
  }
  return latest;
}

/** What the line beneath the feed is told. */
export interface NoteInput {
  securityOnly: boolean;
  /** Events in the window, before the text, kind and run filters. */
  held: number;
  /** The newest sequence the window holds. */
  latest: number | null;
  /** Security mode: how many records were asked of the log. */
  requested: number;
  /** Security mode: how many it returned, or `null` while unanswered or failed. */
  received: number | null;
  /** Unfiltered: how many records the deepest read of the log asked for. */
  read: number;
  /** The run the feed is narrowed to, if any. */
  runId: string | null;
}

/**
 * The sentence that says what the window is a window onto.
 *
 * The security view asks the log, so it can say whether it holds every such
 * record or only the newest. The unfiltered view is the held window, and says
 * how much of the log that is.
 */
export function feedNote(input: NoteInput): string | null {
  const sentences: string[] = [];
  if (input.securityOnly) {
    if (input.received === null) return null;
    if (input.received >= input.requested) {
      sentences.push(
        `These are the newest ${input.requested} security-relevant records in the log; older ones exist.`,
      );
      if (input.requested < DEEP_LOAD) sentences.push(`Load ${DEEP_LOAD} reads further back.`);
    } else {
      sentences.push("Every security-relevant record in the log is loaded.");
    }
  } else {
    const of = input.latest !== null && input.latest > input.held ? ` of ${input.latest}` : "";
    sentences.push(`Holding the newest ${input.held}${of} events in the log.`);
    // Not once it has: pressing it again would re-read the same records.
    if (of !== "" && input.held < EVENT_WINDOW && input.read < DEEP_LOAD) {
      sentences.push(`Load ${DEEP_LOAD} reads further back.`);
    }
  }
  if (input.runId !== null) {
    sentences.push("Only this run's events among them are shown; its trace has the whole run.");
  }
  return sentences.join(" ");
}

/** What an empty feed says, given why it is empty. */
export function emptyMessage(input: {
  securityOnly: boolean;
  filtering: boolean;
  runId: string | null;
  /** The read that would have filled the feed failed. */
  failed: boolean;
}): string {
  if (input.failed) {
    return "The log could not be read, so an empty feed here is not an all-clear.";
  }
  if (input.filtering) {
    return input.securityOnly
      ? "No security-relevant events match this filter."
      : "No events match this filter.";
  }
  if (input.runId !== null) {
    return input.securityOnly
      ? "Nothing security-relevant from this run is loaded."
      : "Nothing from this run is loaded.";
  }
  return input.securityOnly
    ? "Nothing security-relevant has been recorded."
    : "Nothing recorded yet.";
}
