import { describe, expect, it } from "vitest";

import { describeSeries, sparklineGeometry } from "./sparklinePath";

const BOX = { width: 100, height: 32, inset: 2 };

describe("sparklineGeometry", () => {
  it("draws nothing for no data", () => {
    expect(sparklineGeometry([], BOX)).toEqual({ line: "", area: "" });
    expect(sparklineGeometry([Number.NaN, Number.POSITIVE_INFINITY], BOX)).toEqual({
      line: "",
      area: "",
    });
  });

  it("spans the box's width inside its inset, from the floor to the peak", () => {
    const { line } = sparklineGeometry([0, 5, 10], BOX);
    expect(line).toBe("M2 30 L50 16 L98 2");
  });

  it("draws counts from zero, so a small change stays small", () => {
    const { line } = sparklineGeometry([100, 104], BOX);
    // 100 of 104 sits near the top, not at the bottom of the box.
    expect(line).toBe("M2 3.08 L98 2");
  });

  it("keeps a floor below zero when the series goes there", () => {
    const { line } = sparklineGeometry([-10, 10], BOX);
    expect(line).toBe("M2 30 L98 2");
  });

  it("draws a single value, and a series of zeros, as a level line", () => {
    expect(sparklineGeometry([7], BOX).line).toBe("M2 2 L98 2");
    expect(sparklineGeometry([0, 0, 0], BOX).line).toBe("M2 30 L50 30 L98 30");
  });

  it("closes the fill along the baseline", () => {
    const { area } = sparklineGeometry([0, 10], BOX);
    expect(area).toBe("M2 30 L98 2 L98 30 L2 30 Z");
  });

  it("skips values that are not numbers rather than drawing them as zero", () => {
    expect(sparklineGeometry([0, Number.NaN, 10], BOX).line).toBe("M2 30 L98 2");
  });
});

describe("describeSeries", () => {
  it("says where the series went and its range", () => {
    expect(describeSeries([3, 14, 2, 12])).toBe("4 values, from 3 to 12; lowest 2, highest 14");
  });

  it("says when there is one value or none", () => {
    expect(describeSeries([5])).toBe("one value, 5");
    expect(describeSeries([])).toBe("no data");
  });
});
