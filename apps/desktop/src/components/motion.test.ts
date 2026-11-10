import { describe, expect, it } from "vitest";

import {
  cubicBezier,
  decimalsOf,
  easeOut,
  nextPop,
  roundLike,
  spotlightPoint,
  tweenValue,
} from "./motion";

describe("cubicBezier", () => {
  it("starts at 0 and ends at 1, clamping outside the range", () => {
    const ease = cubicBezier(0.65, 0, 0.35, 1);
    expect(ease(0)).toBe(0);
    expect(ease(1)).toBe(1);
    expect(ease(-0.5)).toBe(0);
    expect(ease(2)).toBe(1);
    expect(ease(Number.NaN)).toBe(0);
  });

  it("is the identity for the linear curve", () => {
    const linear = cubicBezier(0, 0, 1, 1);
    for (const x of [0.1, 0.25, 0.5, 0.9]) expect(linear(x)).toBeCloseTo(x, 5);
  });

  it("is symmetric for a symmetric ease-in-out", () => {
    const ease = cubicBezier(0.65, 0, 0.35, 1);
    expect(ease(0.5)).toBeCloseTo(0.5, 5);
    expect(ease(0.2) + ease(0.8)).toBeCloseTo(1, 5);
  });

  it("matches the browser's ease curve at a known point", () => {
    // CSS `ease` is cubic-bezier(.25,.1,.25,1); at x = 0.5 it is ≈ 0.8024.
    expect(cubicBezier(0.25, 0.1, 0.25, 1)(0.5)).toBeCloseTo(0.8024, 3);
  });

  it("never runs backwards on the ease-out the tokens use", () => {
    let previous = 0;
    for (let step = 1; step <= 100; step += 1) {
      const value = easeOut(step / 100);
      expect(value).toBeGreaterThanOrEqual(previous);
      previous = value;
    }
    // Front-loaded: most of the distance is covered in the first third.
    expect(easeOut(1 / 3)).toBeGreaterThan(0.75);
  });
});

describe("tweenValue", () => {
  it("is at the start before any time passes and at the end once it is up", () => {
    expect(tweenValue(10, 20, 0, 600)).toBe(10);
    expect(tweenValue(10, 20, -5, 600)).toBe(10);
    expect(tweenValue(10, 20, 600, 600)).toBe(20);
    expect(tweenValue(10, 20, 9000, 600)).toBe(20);
  });

  it("lands on the target at once when there is no time to move", () => {
    expect(tweenValue(0, 7, 0, 0)).toBe(7);
    expect(tweenValue(0, 7, 10, Number.NaN)).toBe(7);
  });

  it("moves along the easing it is given", () => {
    const linear = (progress: number) => progress;
    expect(tweenValue(0, 100, 150, 600, linear)).toBe(25);
    expect(tweenValue(100, 0, 300, 600, linear)).toBe(50);
  });

  it("stays between its ends", () => {
    for (let elapsed = 0; elapsed <= 600; elapsed += 37) {
      const value = tweenValue(3, 48, elapsed, 600);
      expect(value).toBeGreaterThanOrEqual(3);
      expect(value).toBeLessThanOrEqual(48);
    }
  });
});

describe("rounding a count", () => {
  it("counts in the target's own precision", () => {
    expect(decimalsOf(12)).toBe(0);
    expect(decimalsOf(1.5)).toBe(1);
    expect(decimalsOf(0.125)).toBe(3);
    expect(decimalsOf(1 / 3)).toBe(3);
    expect(roundLike(8.4137, 12)).toBe(8);
    expect(roundLike(1.2345, 2.5)).toBe(1.2);
  });

  it("treats a number it cannot write as a whole", () => {
    expect(decimalsOf(Number.POSITIVE_INFINITY)).toBe(0);
    expect(decimalsOf(Number.NaN)).toBe(0);
  });
});

describe("nextPop", () => {
  it("keeps the same state while the value holds", () => {
    const state = { value: 3, generation: 0 };
    expect(nextPop(state, 3)).toBe(state);
  });

  it("counts each change, so each one pops afresh", () => {
    const first = nextPop({ value: 3, generation: 0 }, 4);
    expect(first).toEqual({ value: 4, generation: 1 });
    expect(nextPop(first, 3)).toEqual({ value: 3, generation: 2 });
  });
});

describe("spotlightPoint", () => {
  it("is the pointer in the box's own coordinates, in whole pixels", () => {
    expect(spotlightPoint(130.4, 260.6, { left: 100, top: 200 })).toEqual({ x: 30, y: 61 });
  });
});
