import { describe, expect, it } from "vitest";

import {
  ARRIVAL_LIMIT,
  RATE_BUCKETS,
  RATE_BUCKET_MS,
  countAfter,
  emptyAction,
  freshArrivals,
  liveStatus,
  needsReveal,
  rateSeries,
  securityCount,
  total,
  unseenLabel,
} from "./feedSignals";

const ids = (...list: string[]) => list.map((id) => ({ id }));

describe("freshArrivals", () => {
  it("finds nothing before the window first held anything", () => {
    expect(freshArrivals(null, ids("a", "b"))).toEqual(new Set());
    expect(freshArrivals(new Set(), ids("a", "b"))).toEqual(new Set());
  });

  it("finds the events after the newest one already known", () => {
    expect(freshArrivals(new Set(["a", "b"]), ids("a", "b", "c", "d"))).toEqual(
      new Set(["c", "d"]),
    );
  });

  it("does not count history read from further back as arriving", () => {
    // A deeper read puts older events before the known ones.
    expect(freshArrivals(new Set(["c", "d"]), ids("a", "b", "c", "d"))).toEqual(new Set());
  });

  it("treats a window with nothing known in it as replaced, not added to", () => {
    expect(freshArrivals(new Set(["x"]), ids("a", "b"))).toEqual(new Set());
  });

  it("treats a burst larger than the limit as a read, not arrivals", () => {
    const many = Array.from({ length: ARRIVAL_LIMIT + 1 }, (_, index) => `n${index}`);
    expect(freshArrivals(new Set(["a"]), ids("a", ...many))).toEqual(new Set());
    const few = many.slice(0, ARRIVAL_LIMIT);
    expect(freshArrivals(new Set(["a"]), ids("a", ...few)).size).toBe(ARRIVAL_LIMIT);
  });

  it("still finds arrivals after the oldest events have left the window", () => {
    expect(freshArrivals(new Set(["a", "b", "c"]), ids("b", "c", "d"))).toEqual(new Set(["d"]));
  });
});

describe("countAfter", () => {
  it("counts what follows the anchor", () => {
    expect(countAfter(ids("a", "b", "c"), "a")).toBe(2);
    expect(countAfter(ids("a", "b", "c"), "c")).toBe(0);
  });

  it("gives zero rather than a guess when there is no anchor or it has gone", () => {
    expect(countAfter(ids("a", "b"), null)).toBe(0);
    expect(countAfter(ids("a", "b"), "z")).toBe(0);
  });

  it("is worded for one and for many", () => {
    expect(unseenLabel(1)).toBe("1 new event");
    expect(unseenLabel(4)).toBe("4 new events");
  });
});

describe("rateSeries", () => {
  const now = Date.UTC(2026, 10, 9, 12, 0, 0);
  const at = (msAgo: number) => ({ at: new Date(now - msAgo).toISOString() });

  it("has one bucket per five minutes of the last hour, oldest first", () => {
    const series = rateSeries([], now);
    expect(series).toHaveLength(RATE_BUCKETS);
    expect(RATE_BUCKETS * RATE_BUCKET_MS).toBe(60 * 60_000);
  });

  it("files each event in the bucket it happened in", () => {
    const series = rateSeries([at(60_000), at(2 * 60_000), at(7 * 60_000), at(58 * 60_000)], now);
    expect(series.at(-1)).toBe(2);
    expect(series.at(-2)).toBe(1);
    expect(series[0]).toBe(1);
    expect(total(series)).toBe(4);
  });

  it("leaves out what is older than the window or has no readable time", () => {
    const series = rateSeries([at(61 * 60_000), { at: "not a time" }], now);
    expect(total(series)).toBe(0);
  });

  it("counts an event stamped a little in the future in the newest bucket", () => {
    expect(rateSeries([at(-30_000)], now).at(-1)).toBe(1);
  });
});

describe("securityCount", () => {
  it("counts the security-relevant events", () => {
    expect(
      securityCount([
        { security_relevant: true },
        { security_relevant: false },
        { security_relevant: true },
      ]),
    ).toBe(2);
  });
});

describe("liveStatus", () => {
  it("is live and pulsing while a runtime's stream flows", () => {
    expect(liveStatus({ fixtures: false, loaded: true, failed: false })).toEqual({
      label: "Live",
      tone: "ok",
      pulse: true,
      note: null,
    });
  });

  it("says fixtures, in the attention hue, when the stream is a replay", () => {
    const status = liveStatus({ fixtures: true, loaded: true, failed: false });
    expect(status.note).toBe("fixtures");
    expect(status.tone).toBe("warn");
  });

  it("stops pulsing when the stream is not answering, fixtures or not", () => {
    for (const fixtures of [true, false]) {
      const status = liveStatus({ fixtures, loaded: true, failed: true });
      expect(status).toMatchObject({ label: "Not answering", tone: "warn", pulse: false });
    }
  });

  it("does not claim to be live before history has been read", () => {
    expect(liveStatus({ fixtures: false, loaded: false, failed: false })).toMatchObject({
      label: "Connecting",
      pulse: false,
    });
  });
});

describe("emptyAction", () => {
  const base = { securityOnly: false, filtering: false, runId: null, failed: false };

  it("offers to read again before anything else when the read failed", () => {
    expect(emptyAction({ ...base, failed: true, filtering: true, runId: "r" })).toBe("retry");
  });

  it("offers to clear a filter that is narrowing", () => {
    expect(emptyAction({ ...base, filtering: true, runId: "r", securityOnly: true })).toBe("clear");
  });

  it("offers every run, then every event, then somewhere to start work", () => {
    expect(emptyAction({ ...base, runId: "r", securityOnly: true })).toBe("every-run");
    expect(emptyAction({ ...base, securityOnly: true })).toBe("every-event");
    expect(emptyAction(base)).toBe("tasks");
  });
});

describe("needsReveal", () => {
  const view = { top: 56, bottom: 900 };

  it("leaves alone what is already wholly in view", () => {
    expect(needsReveal({ top: 120, bottom: 160 }, view)).toBe(false);
  });

  it("brings in what is below the view, above it, or under the pinned header", () => {
    expect(needsReveal({ top: 950, bottom: 990 }, view)).toBe(true);
    expect(needsReveal({ top: -80, bottom: -40 }, view)).toBe(true);
    expect(needsReveal({ top: 30, bottom: 70 }, view)).toBe(true);
    expect(needsReveal({ top: 880, bottom: 920 }, view)).toBe(true);
  });
});
