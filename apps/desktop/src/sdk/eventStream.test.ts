import { describe, expect, it } from "vitest";

import type { EventView } from "../bindings/EventView";
import {
  EVENT_WINDOW,
  type EventWindows,
  SECURITY_WINDOW,
  feedEvents,
  instant,
  mergeEvents,
  orderEvents,
} from "./eventStream";

function event(
  id: string,
  at: string,
  sequence: number | null = null,
  securityRelevant = false,
): EventView {
  return {
    id,
    sequence,
    at,
    kind: securityRelevant ? "tool.refused" : "tool.completed",
    run_id: null,
    task_id: null,
    summary: id,
    security_relevant: securityRelevant,
  };
}

const EMPTY: EventWindows = { all: [], security: [] };

function ids(list: readonly EventView[]): string[] {
  return list.map((each) => each.id);
}

describe("merging by id", () => {
  it("does not duplicate an event delivered twice", () => {
    let windows = mergeEvents(EMPTY, [event("a", "2026-09-29T10:00:00Z")]);
    windows = mergeEvents(windows, [event("a", "2026-09-29T10:00:00Z")]);
    expect(ids(windows.all)).toEqual(["a"]);
  });

  it("prefers the stored copy, which carries the sequence, over the streamed one", () => {
    let windows = mergeEvents(EMPTY, [event("a", "2026-09-29T10:00:00Z", 7)]);
    windows = mergeEvents(windows, [event("a", "2026-09-29T10:00:00Z", null)]);
    expect(windows.all[0]?.sequence).toBe(7);
  });

  it("replaces a streamed copy when the stored copy arrives", () => {
    let windows = mergeEvents(EMPTY, [event("a", "2026-09-29T10:00:00Z", null)]);
    windows = mergeEvents(windows, [event("a", "2026-09-29T10:00:00Z", 7)]);
    expect(windows.all).toHaveLength(1);
    expect(windows.all[0]?.sequence).toBe(7);
  });
});

describe("ordering", () => {
  it("orders stored events by sequence, even against their clocks", () => {
    const ordered = orderEvents([
      event("second", "2026-09-29T10:00:00Z", 2),
      event("first", "2026-09-29T10:00:05Z", 1),
    ]);
    expect(ids(ordered)).toEqual(["first", "second"]);
  });

  it("orders streamed events by time rather than treating a missing sequence as zero", () => {
    const ordered = orderEvents([
      event("live", "2026-09-29T10:00:10Z", null),
      event("stored-1", "2026-09-29T10:00:00Z", 1),
      event("stored-2", "2026-09-29T10:00:05Z", 2),
    ]);
    expect(ids(ordered)).toEqual(["stored-1", "stored-2", "live"]);
  });

  it("places streamed events among stored ones by time", () => {
    const ordered = orderEvents([
      event("stored-2", "2026-09-29T10:00:10Z", 2),
      event("live", "2026-09-29T10:00:05Z", null),
      event("stored-1", "2026-09-29T10:00:00Z", 1),
    ]);
    expect(ids(ordered)).toEqual(["stored-1", "live", "stored-2"]);
  });

  it("keeps precision below a millisecond", () => {
    const ordered = orderEvents([
      event("b", "2026-09-29T10:00:00.000200+00:00"),
      event("a", "2026-09-29T10:00:00.000100+00:00"),
    ]);
    expect(ids(ordered)).toEqual(["a", "b"]);
  });

  it("reads a stamp with no fraction as earlier than one with a fraction", () => {
    expect(instant("2026-09-29T10:00:00Z")).toBeLessThan(instant("2026-09-29T10:00:00.1Z"));
  });

  it("sorts an unparseable stamp last instead of scrambling the order", () => {
    const ordered = orderEvents([
      event("bad", "not a time"),
      event("b", "2026-09-29T10:00:01Z"),
      event("a", "2026-09-29T10:00:00Z"),
    ]);
    expect(ids(ordered)).toEqual(["a", "b", "bad"]);
  });
});

describe("the windows", () => {
  it("holds the full window size the Activity screen needs", () => {
    expect(EVENT_WINDOW).toBe(2000);
    expect(SECURITY_WINDOW).toBeGreaterThanOrEqual(EVENT_WINDOW);
  });

  it("drops the oldest events beyond the limit", () => {
    const incoming = [1, 2, 3, 4, 5].map((n) => event(`e${n}`, `2026-09-29T10:00:0${n}Z`, n));
    const windows = mergeEvents(EMPTY, incoming, { all: 3, security: 3 });
    expect(ids(windows.all)).toEqual(["e3", "e4", "e5"]);
  });

  it("keeps a refusal that ordinary traffic pushed out of the main window", () => {
    const limits = { all: 3, security: 3 };
    let windows = mergeEvents(
      EMPTY,
      [event("refused", "2026-09-29T10:00:00Z", 1, true)],
      limits,
    );
    windows = mergeEvents(
      windows,
      [1, 2, 3].map((n) => event(`ok${n}`, `2026-09-29T10:00:0${n}Z`, n + 1)),
      limits,
    );

    expect(ids(windows.all)).not.toContain("refused");
    expect(ids(feedEvents(windows, { securityOnly: true, follow: true }))).toEqual(["refused"]);
    expect(ids(feedEvents(windows, { securityOnly: false, follow: true }))).toEqual([
      "ok1",
      "ok2",
      "ok3",
    ]);
  });

  it("holds every refusal the main window holds, at the real sizes", () => {
    const incoming = Array.from({ length: EVENT_WINDOW + 500 }, (_, n) =>
      event(`e${n}`, new Date(Date.UTC(2026, 8, 29, 10, 0, 0, n)).toISOString(), n, n % 3 === 0),
    );
    const windows = mergeEvents(EMPTY, incoming);
    const security = new Set(ids(windows.security));
    const missing = windows.all.filter((each) => each.security_relevant && !security.has(each.id));
    expect(windows.all).toHaveLength(EVENT_WINDOW);
    expect(missing).toEqual([]);
  });
});
