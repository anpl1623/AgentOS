import { describe, expect, it } from "vitest";

import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { callsDelta, dailyCalls, perDayCaption, perDayLabel, toolSeries } from "./usageModel";

function usage(tool: string, daily: readonly number[]): ToolUsageView {
  const calls = daily.reduce((sum, count) => sum + count, 0);
  return {
    tool,
    calls,
    executed: calls,
    denied: 0,
    approval_denied: 0,
    failed: 0,
    under_taint: 0,
    approvals: 0,
    total_duration_ms: 0,
    last_used_at: null,
    risk: "low",
    returns_untrusted_data: false,
    daily: [...daily],
  };
}

/** A report from a runtime that answers without the series. */
function withoutSeries(tool: string): ToolUsageView {
  const { daily: _, ...rest } = usage(tool, [3]);
  return rest as ToolUsageView;
}

describe("toolSeries", () => {
  it("gives a tool's calls per day as the runtime sent them", () => {
    expect(toolSeries(usage("t", [0, 2, 1]))).toEqual([0, 2, 1]);
  });

  it("leaves out a series that is missing, empty or not counts, rather than drawing zeros", () => {
    expect(toolSeries(withoutSeries("old"))).toBeNull();
    expect(toolSeries(usage("t", []))).toBeNull();
    expect(toolSeries({ ...usage("t", [1]), daily: [1, Number.NaN] })).toBeNull();
    expect(toolSeries({ ...usage("t", [1]), daily: [1, -2] })).toBeNull();
  });
});

describe("dailyCalls", () => {
  it("adds every tool's calls day by day, keeping today last", () => {
    const report = [usage("a", [1, 0, 4]), usage("b", [2, 3, 0]), usage("idle", [0, 0, 0])];
    expect(dailyCalls(report)).toEqual([3, 3, 4]);
  });

  it("lines a shorter series up by today, not by its first day", () => {
    // Today's single count belongs on today, not on the window's first day.
    expect(dailyCalls([usage("a", [1, 1, 1]), usage("b", [5])])).toEqual([1, 1, 6]);
  });

  it("has no line to draw when no tool sent a series", () => {
    expect(dailyCalls([withoutSeries("old")])).toBeNull();
    expect(dailyCalls([])).toBeNull();
  });

  it("draws the tools that did send one, beside one that did not", () => {
    expect(dailyCalls([withoutSeries("old"), usage("new", [2, 1])])).toEqual([2, 1]);
  });
});

describe("the words for a per-day line", () => {
  it("names the measure, its unit, its window and the day still under way", () => {
    expect(perDayLabel("Tool calls", 7)).toBe(
      "Tool calls per day over the last 7 days, ending with today so far",
    );
    expect(perDayLabel("Tool calls", 1)).toBe("Tool calls today so far");
  });

  it("captions the line with its unit and window", () => {
    expect(perDayCaption(30)).toBe("per day, last 30 days");
    expect(perDayCaption(1)).toBe("today so far");
  });
});

describe("callsDelta", () => {
  // Today is a day still under way: the comparison is with the whole days
  // before it, and only a day already ahead of them can be called up.
  it("says so once today has passed the days before it", () => {
    expect(callsDelta(21, [10, 12, 9, 11, 8, 10, 4])).toEqual({
      text: "above 10/day before today",
      direction: "up",
      sentiment: "neutral",
    });
  });

  it("does not count today among the days it is compared with", () => {
    // Counted in, today's own 40 would have made the average 20.
    expect(callsDelta(40, [10, 10, 40])?.text).toBe("above 10/day before today");
  });

  it("says nothing of a day that is behind or level so far, which may yet catch up", () => {
    expect(callsDelta(2, [10, 12, 9, 11, 8, 10, 2])).toBeNull();
    expect(callsDelta(11, [10, 12, 9, 11, 8, 10, 11])).toBeNull();
  });

  it("writes a small average to one decimal", () => {
    expect(callsDelta(3, [12, ...Array<number>(28).fill(0), 3])?.text).toBe(
      "above 0.4/day before today",
    );
    expect(callsDelta(5, [2, 5])?.text).toBe("above 2/day before today");
  });

  it("has nothing to compare with before any earlier day had a call", () => {
    expect(callsDelta(5, [0, 0, 5])).toBeNull();
    expect(callsDelta(5, [5])).toBeNull();
    expect(callsDelta(5, null)).toBeNull();
    expect(callsDelta(Number.NaN, [4, 4, 4])).toBeNull();
  });
});
