import { describe, expect, it } from "vitest";

import tokens from "../styles/tokens.css?raw";
import {
  LANE_HEIGHT,
  MIN_SPAN_PERCENT,
  elapsed,
  offsetLabel,
  percent,
  position,
  spanGeometry,
  ticks,
} from "./charts";

const domain = { start: 1_000, end: 11_000 };

describe("position", () => {
  it("places an instant across the domain as a percentage", () => {
    expect(position(1_000, domain)).toBe(0);
    expect(position(6_000, domain)).toBe(50);
    expect(position(11_000, domain)).toBe(100);
  });

  it("clamps what falls outside, and survives an empty or unreadable domain", () => {
    expect(position(0, domain)).toBe(0);
    expect(position(20_000, domain)).toBe(100);
    expect(position(5, { start: 5, end: 5 })).toBe(0);
    expect(position(Number.NaN, domain)).toBe(0);
  });
});

describe("spanGeometry", () => {
  it("draws a span as wide as it lasted", () => {
    expect(spanGeometry(1_000, 3_500, domain)).toEqual({ x: 0, width: 25 });
  });

  it("widens a sliver to the minimum, so a two-millisecond call stays a target", () => {
    const { width } = spanGeometry(6_000, 6_002, domain);
    expect(width).toBe(MIN_SPAN_PERCENT);
  });

  it("keeps a minimum-width span at the very end inside the chart", () => {
    const { x, width } = spanGeometry(11_000, 11_000, domain);
    expect(x + width).toBeLessThanOrEqual(100);
    expect(width).toBe(MIN_SPAN_PERCENT);
  });

  it("never draws a negative width for an interval written backwards", () => {
    expect(spanGeometry(6_000, 2_000, domain).width).toBe(MIN_SPAN_PERCENT);
  });
});

describe("ticks", () => {
  it("chooses an interval a person counts in, with at most six ticks after zero", () => {
    expect(ticks(90_000)).toEqual([0, 15_000, 30_000, 45_000, 60_000, 75_000, 90_000]);
    const hour = ticks(60 * 60_000);
    expect(hour[1]).toBe(10 * 60_000);
    expect(hour.length - 1).toBeLessThanOrEqual(6);
  });

  it("never runs past the end", () => {
    for (const length of [1, 999, 7_300, 61_000, 3_601_000]) {
      const marks = ticks(length);
      expect(marks.at(-1)).toBeLessThanOrEqual(length);
      expect(marks[0]).toBe(0);
    }
  });

  it("returns only the origin for a length it cannot divide", () => {
    expect(ticks(0)).toEqual([0]);
    expect(ticks(Number.NaN)).toEqual([0]);
  });
});

describe("offsetLabel", () => {
  it("writes offsets the way they are said", () => {
    expect(offsetLabel(0)).toBe("0");
    expect(offsetLabel(45_000)).toBe("45s");
    expect(offsetLabel(120_000)).toBe("2m");
    expect(offsetLabel(150_000)).toBe("2m 30s");
    expect(offsetLabel(75 * 60_000)).toBe("1h 15m");
    expect(offsetLabel(2 * 60 * 60_000)).toBe("2h");
  });
});

describe("percent", () => {
  it("is an SVG length without float noise", () => {
    expect(percent(12.5)).toBe("12.5%");
    expect(percent(1 / 3)).toBe("0.333%");
  });
});

describe("elapsed", () => {
  it("writes long spans in days and hours, not thousands of minutes", () => {
    expect(elapsed(850)).toBe("850ms");
    expect(elapsed(42_000)).toBe("42s");
    expect(elapsed(11 * 60_000)).toBe("11m");
    expect(elapsed(200 * 60_000)).toBe("3h 20m");
    expect(elapsed(52 * 3_600_000)).toBe("2d 4h");
    expect(elapsed(48 * 3_600_000)).toBe("2d");
  });
});

describe("LANE_HEIGHT", () => {
  // SVG geometry attributes cannot read a custom property, so the lane height
  // is written twice. The track drawn in SVG and the row the stylesheet sizes
  // around it must not drift apart.
  it("is the --chart-row token", () => {
    expect(/--chart-row:\s*([^;]+);/.exec(tokens)?.[1]?.trim()).toBe(`${LANE_HEIGHT}px`);
  });
});
