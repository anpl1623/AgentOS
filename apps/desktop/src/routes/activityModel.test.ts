import { describe, expect, it } from "vitest";

import type { EventView } from "../bindings/EventView";
import { EVENT_WINDOW } from "../sdk/eventStream";
import {
  DEEP_LOAD,
  FOLLOW_THRESHOLD_PX,
  HASH_PREFIX,
  NO_QUERY,
  UNKNOWN_DAY,
  canOpen,
  dayKey,
  dayLabel,
  emptyMessage,
  feedNote,
  filterFeed,
  followAfterScroll,
  groupByDay,
  isFiltering,
  kindOptions,
  latestSequence,
  mergeFeed,
  shortHash,
  toJsonl,
} from "./activityModel";

/** A local wall-clock time as the runtime would write it. */
function local(year: number, month: number, day: number, hour = 12, minute = 0): string {
  return new Date(year, month - 1, day, hour, minute).toISOString();
}

function event(id: string, extra: Partial<EventView> = {}): EventView {
  return {
    id,
    sequence: null,
    at: local(2026, 10, 6),
    kind: "tool.execution.completed",
    run_id: "run-1",
    task_id: "task-1",
    summary: "browser.navigate",
    security_relevant: false,
    ...extra,
  };
}

describe("mergeFeed", () => {
  it("keeps one copy of an event known twice, and it is the stored one", () => {
    const streamed = event("a", { sequence: null });
    const stored = event("a", { sequence: 7 });
    // Whichever source answers last, the copy that can be opened survives.
    expect(mergeFeed([[stored], [streamed]])).toEqual([stored]);
    expect(mergeFeed([[streamed], [stored]])).toEqual([stored]);
  });

  it("orders by the chain, not by arrival", () => {
    const merged = mergeFeed([
      [event("c", { sequence: 3 }), event("a", { sequence: 1 })],
      [event("b", { sequence: 2 })],
    ]);
    expect(merged.map((e) => e.id)).toEqual(["a", "b", "c"]);
  });

  it("keeps the newest events when there are more than the limit", () => {
    const many = Array.from({ length: 5 }, (_, i) => event(`e${i}`, { sequence: i + 1 }));
    expect(mergeFeed([many], 2).map((e) => e.id)).toEqual(["e3", "e4"]);
  });

  it("holds the shared window by default", () => {
    const many = Array.from({ length: EVENT_WINDOW + 10 }, (_, i) =>
      event(`e${i}`, { sequence: i + 1 }),
    );
    expect(mergeFeed([many])).toHaveLength(EVENT_WINDOW);
  });
});

describe("filterFeed", () => {
  const feed = [
    event("1", { kind: "permission.denied", summary: "filesystem.read" }),
    event("2", { kind: "tool.execution.completed", summary: "Browser.Navigate" }),
    event("3", { kind: "permission.granted", summary: "browser.type", run_id: "run-2" }),
  ];

  it("matches text against kind and summary, ignoring case", () => {
    expect(filterFeed(feed, { ...NO_QUERY, text: "PERMISSION" }).map((e) => e.id)).toEqual([
      "1",
      "3",
    ]);
    expect(filterFeed(feed, { ...NO_QUERY, text: "browser.nav" }).map((e) => e.id)).toEqual(["2"]);
  });

  it("ignores surrounding whitespace in the text", () => {
    expect(filterFeed(feed, { ...NO_QUERY, text: "   " })).toHaveLength(3);
  });

  it("matches a kind exactly", () => {
    expect(filterFeed(feed, { ...NO_QUERY, kind: "permission" })).toEqual([]);
    expect(filterFeed(feed, { ...NO_QUERY, kind: "permission.denied" }).map((e) => e.id)).toEqual([
      "1",
    ]);
  });

  it("narrows to one run", () => {
    expect(filterFeed(feed, { ...NO_QUERY, runId: "run-2" }).map((e) => e.id)).toEqual(["3"]);
  });

  it("combines every part of the query", () => {
    const query = { text: "browser", kind: "permission.granted", runId: "run-1" };
    expect(filterFeed(feed, query)).toEqual([]);
  });

  it("counts only text and kind as filtering, since the run has its own chip", () => {
    expect(isFiltering(NO_QUERY)).toBe(false);
    expect(isFiltering({ ...NO_QUERY, runId: "run-1" })).toBe(false);
    expect(isFiltering({ ...NO_QUERY, text: " x " })).toBe(true);
    expect(isFiltering({ ...NO_QUERY, kind: "a.b" })).toBe(true);
  });
});

describe("kindOptions", () => {
  it("lists each kind once, sorted", () => {
    const feed = [
      event("1", { kind: "b.x" }),
      event("2", { kind: "a.y" }),
      event("3", { kind: "b.x" }),
    ];
    expect(kindOptions(feed, "")).toEqual(["a.y", "b.x"]);
  });

  it("keeps the chosen kind listed after it has left the window", () => {
    expect(kindOptions([event("1", { kind: "b.x" })], "z.gone")).toEqual(["b.x", "z.gone"]);
  });
});

describe("days", () => {
  it("files an event under its local calendar day", () => {
    expect(dayKey(local(2026, 10, 5, 23, 59))).toBe("2026-10-05");
    expect(dayKey(local(2026, 10, 6, 0, 1))).toBe("2026-10-06");
  });

  it("files an unreadable timestamp under an unknown day rather than throwing", () => {
    expect(dayKey("not a time")).toBe(UNKNOWN_DAY);
    expect(dayLabel(UNKNOWN_DAY, new Date())).toBe("Date unknown");
  });

  it("starts a group wherever the day changes between neighbours", () => {
    const feed = [
      event("1", { at: local(2026, 10, 5, 23, 58) }),
      event("2", { at: local(2026, 10, 5, 23, 59) }),
      event("3", { at: local(2026, 10, 6, 0, 1) }),
    ];
    const groups = groupByDay(feed);
    expect(groups.map((g) => [g.day, g.events.map((e) => e.id)])).toEqual([
      ["2026-10-05", ["1", "2"]],
      ["2026-10-06", ["3"]],
    ]);
  });

  it("gives a recurring day its own group with a distinct id", () => {
    // The chain's order can disagree with the clock; each run of a day is
    // separated, and the ids must still key a list.
    const feed = [
      event("1", { at: local(2026, 10, 5) }),
      event("2", { at: local(2026, 10, 6) }),
      event("3", { at: local(2026, 10, 5) }),
    ];
    const groups = groupByDay(feed);
    expect(groups.map((g) => g.day)).toEqual(["2026-10-05", "2026-10-06", "2026-10-05"]);
    expect(new Set(groups.map((g) => g.id)).size).toBe(3);
  });

  it("says today and yesterday, and always the date as well", () => {
    const now = new Date(2026, 9, 6, 9, 0);
    const today = dayLabel("2026-10-06", now);
    const yesterday = dayLabel("2026-10-05", now);
    const earlier = dayLabel("2026-10-01", now);
    expect(today.startsWith("Today · ")).toBe(true);
    expect(yesterday.startsWith("Yesterday · ")).toBe(true);
    expect(earlier).not.toMatch(/Today|Yesterday/);
    expect(new Set([today.slice(8), yesterday.slice(12), earlier]).size).toBe(3);
  });

  it("names the year only when it is not this one", () => {
    const now = new Date(2026, 9, 6);
    expect(dayLabel("2025-10-06", now)).toMatch(/2025/);
    expect(dayLabel("2026-10-01", now)).not.toMatch(/2026/);
  });
});

describe("followAfterScroll", () => {
  it("turns off when the operator scrolls further than the threshold from the end", () => {
    expect(followAfterScroll(true, FOLLOW_THRESHOLD_PX + 1, true)).toBe(false);
  });

  it("turns back on when the operator returns to within the threshold", () => {
    expect(followAfterScroll(false, FOLLOW_THRESHOLD_PX, true)).toBe(true);
    expect(followAfterScroll(false, 0, true)).toBe(true);
  });

  it("turns off when the operator scrolls past the end to read a record", () => {
    expect(followAfterScroll(true, -(FOLLOW_THRESHOLD_PX + 1), true)).toBe(false);
  });

  it("is not changed by a scroll the screen made itself", () => {
    expect(followAfterScroll(true, 5_000, false)).toBe(true);
    expect(followAfterScroll(false, 0, false)).toBe(false);
  });
});

describe("records", () => {
  it("opens only events the log has stored", () => {
    expect(canOpen(event("a", { sequence: 4 }))).toBe(true);
    expect(canOpen(event("a", { sequence: null }))).toBe(false);
  });

  it("shortens a hash to its prefix and marks the cut", () => {
    const hash = "0123456789abcdef".repeat(4);
    expect(shortHash(hash)).toBe(`${hash.slice(0, HASH_PREFIX)}…`);
    expect(shortHash("short")).toBe("short");
  });

  it("finds the newest stored sequence, ignoring streamed events", () => {
    expect(latestSequence([])).toBeNull();
    expect(latestSequence([event("a")])).toBeNull();
    expect(
      latestSequence([event("a", { sequence: 9 }), event("b"), event("c", { sequence: 3 })]),
    ).toBe(9);
  });
});

describe("toJsonl", () => {
  it("writes one parseable object per line, ending in a newline", () => {
    const feed = [event("a", { summary: "line\nbreak" }), event("b")];
    const text = toJsonl(feed);
    expect(text.endsWith("\n")).toBe(true);
    const lines = text.trimEnd().split("\n");
    expect(lines).toHaveLength(2);
    expect(lines.map((line) => JSON.parse(line) as EventView)).toEqual(feed);
  });

  it("writes nothing for nothing", () => {
    expect(toJsonl([])).toBe("");
  });
});

describe("feedNote", () => {
  const base = {
    securityOnly: false,
    held: 500,
    latest: 12_000,
    requested: 500,
    received: null,
    read: 500,
    runId: null,
  };

  it("says how much of the log the unfiltered window holds", () => {
    expect(feedNote(base)).toBe(
      `Holding the newest 500 of 12000 events in the log. Load ${DEEP_LOAD} reads further back.`,
    );
  });

  it("does not offer to read further once the window is full", () => {
    expect(feedNote({ ...base, held: EVENT_WINDOW })).not.toMatch(/Load/);
  });

  it("does not offer the deeper read once it has been made", () => {
    // A log of 5000 read 1000 deep holds about 1000, short of the window,
    // and pressing Load again would only re-read the same newest records.
    expect(feedNote({ ...base, held: 1000, latest: 5000, read: DEEP_LOAD })).toBe(
      "Holding the newest 1000 of 5000 events in the log.",
    );
  });

  it("does not claim a remainder when the whole log is held", () => {
    expect(feedNote({ ...base, held: 40, latest: 40 })).toBe(
      "Holding the newest 40 events in the log.",
    );
  });

  it("says when the security view holds only the newest records", () => {
    const note = feedNote({ ...base, securityOnly: true, requested: 500, received: 500 });
    expect(note).toMatch(/newest 500 security-relevant records/);
    expect(note).toMatch(/older ones exist/);
    expect(note).toMatch(/Load 1000/);
  });

  it("says when the security view holds every such record in the log", () => {
    expect(feedNote({ ...base, securityOnly: true, received: 12 })).toBe(
      "Every security-relevant record in the log is loaded.",
    );
  });

  it("claims nothing about the security view before the log has answered", () => {
    expect(feedNote({ ...base, securityOnly: true, received: null })).toBeNull();
  });

  it("says a run's events are limited to what is loaded", () => {
    expect(feedNote({ ...base, runId: "run-1" })).toMatch(/its trace has the whole run/);
  });
});

describe("emptyMessage", () => {
  const base = { securityOnly: true, filtering: false, runId: null, failed: false };

  it("never reads as an all-clear when the read failed", () => {
    expect(emptyMessage({ ...base, failed: true })).toMatch(/not an all-clear/);
    expect(emptyMessage({ ...base, securityOnly: false, failed: true })).toMatch(
      /not an all-clear/,
    );
  });

  it("names security-relevant events, not only refusals", () => {
    expect(emptyMessage(base)).toBe("Nothing security-relevant has been recorded.");
  });

  it("blames the filter when a filter is narrowing", () => {
    expect(emptyMessage({ ...base, filtering: true })).toMatch(/match this filter/);
    expect(emptyMessage({ ...base, securityOnly: false, filtering: true })).toBe(
      "No events match this filter.",
    );
  });

  it("speaks of what is loaded when narrowed to a run", () => {
    expect(emptyMessage({ ...base, securityOnly: false, runId: "run-1" })).toBe(
      "Nothing from this run is loaded.",
    );
  });
});
