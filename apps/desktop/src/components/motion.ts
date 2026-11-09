/**
 * Motion in script, for what a stylesheet cannot animate: a number counting
 * to its value, a badge that pops when its count changes, and the light that
 * follows the pointer across a card.
 *
 * Every helper honours reduced motion itself, as the stylesheets do: a person
 * who asked their system for less motion sees the final value at once. The
 * arithmetic is pure and tested; the hooks only schedule it.
 */

import { useEffect, useRef, useState } from "react";

/** Whether the operator has asked the system for less motion. */
export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * A CSS `cubic-bezier()` timing function, as a function of progress.
 *
 * Solved for x by Newton's method, with bisection when the slope is too flat
 * to trust, so a tween in script moves on exactly the curve its CSS
 * neighbours use.
 */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): (x: number) => number {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const sampleX = (t: number) => ((ax * t + bx) * t + cx) * t;
  const sampleY = (t: number) => ((ay * t + by) * t + cy) * t;
  const slopeX = (t: number) => (3 * ax * t + 2 * bx) * t + cx;

  return (x: number) => {
    if (!(x > 0)) return 0;
    if (x >= 1) return 1;
    let t = x;
    for (let i = 0; i < 8; i += 1) {
      const error = sampleX(t) - x;
      if (Math.abs(error) < 1e-7) return sampleY(t);
      const slope = slopeX(t);
      if (Math.abs(slope) < 1e-6) break;
      t -= error / slope;
    }
    let low = 0;
    let high = 1;
    t = x;
    while (high - low > 1e-7) {
      if (sampleX(t) < x) low = t;
      else high = t;
      t = (low + high) / 2;
    }
    return sampleY(t);
  };
}

/** `--ease-out` in tokens.css: a quick start that settles gently. */
export const easeOut = cubicBezier(0.16, 1, 0.3, 1);

/** How long a count-up takes, about `--dur-4` plus the settle a number needs to be read. */
export const COUNT_UP_MS = 600;

/**
 * The value a tween from `from` to `to` has reached `elapsed` milliseconds into
 * a `duration`. Clamped at both ends, so a late frame lands exactly on `to`.
 */
export function tweenValue(
  from: number,
  to: number,
  elapsed: number,
  duration: number,
  ease: (progress: number) => number = easeOut,
): number {
  if (!(duration > 0) || elapsed >= duration) return to;
  if (!(elapsed > 0)) return from;
  return from + (to - from) * ease(elapsed / duration);
}

/** How many decimal places `value` is written with, up to three. */
export function decimalsOf(value: number): number {
  if (!Number.isFinite(value) || Number.isInteger(value)) return 0;
  const written = String(value);
  const point = written.indexOf(".");
  if (point === -1 || written.includes("e")) return 3;
  return Math.min(written.length - point - 1, 3);
}

/**
 * `value` rounded to the precision `target` is written with, so a count to 12
 * passes through 7 and 9, never 8.4137.
 */
export function roundLike(value: number, target: number): number {
  const factor = 10 ** decimalsOf(target);
  return Math.round(value * factor) / factor;
}

/**
 * A number that counts to `target`: up from zero when first drawn, then from
 * wherever it stands to each new target. An interrupted count turns from the
 * value on screen, so it never jumps backwards.
 *
 * Reduced motion shows the target at once. The count is also settled by a
 * timer, because frames stop in a hidden window and a figure must not be left
 * part-way for whoever looks next.
 */
export function useCountUp(target: number, duration = COUNT_UP_MS): number {
  const [shown, setShown] = useState(() =>
    prefersReducedMotion() || !Number.isFinite(target) ? target : 0,
  );
  const current = useRef(shown);

  useEffect(() => {
    const from = current.current;
    const land = (value: number) => {
      current.current = value;
      setShown(value);
    };
    if (from === target) return;
    if (prefersReducedMotion() || !Number.isFinite(from) || !Number.isFinite(target)) {
      land(target);
      return;
    }

    let frame = 0;
    const start = performance.now();
    const step = (now: number) => {
      const elapsed = now - start;
      land(roundLike(tweenValue(from, target, elapsed, duration), target));
      if (elapsed < duration) frame = window.requestAnimationFrame(step);
    };
    frame = window.requestAnimationFrame(step);
    const settle = window.setTimeout(() => {
      window.cancelAnimationFrame(frame);
      land(target);
    }, duration + 120);

    return () => {
      window.cancelAnimationFrame(frame);
      window.clearTimeout(settle);
    };
  }, [target, duration]);

  return shown;
}

/** A value and how many times it has changed since it was first drawn. */
export interface PopState {
  value: number;
  generation: number;
}

/** The pop state after seeing `value`; the same object when nothing changed. */
export function nextPop(state: PopState, value: number): PopState {
  return Object.is(state.value, value) ? state : { value, generation: state.generation + 1 };
}

/**
 * Pop a badge when its count changes, never when it is first drawn.
 *
 * Use `key` as the badge's React key, so each change mounts a fresh element
 * and its `.pop` animation plays from the start, and add `.pop` while
 * `popping` is true.
 */
export function usePop(value: number): { key: number; popping: boolean } {
  const [state, setState] = useState<PopState>(() => ({ value, generation: 0 }));
  const next = nextPop(state, value);
  // Adjusting state while rendering, from the value the render was given: the
  // pattern React documents for state derived from a changing prop.
  if (next !== state) setState(next);
  return { key: next.generation, popping: next.generation > 0 };
}

/** Where the pointer is, in the box's own coordinates. */
export function spotlightPoint(
  clientX: number,
  clientY: number,
  box: { left: number; top: number },
): { x: number; y: number } {
  return { x: Math.round(clientX - box.left), y: Math.round(clientY - box.top) };
}

/**
 * Light the hovered `[data-spotlight]` element where the pointer is.
 *
 * One delegated listener for the whole document, installed once by the shell,
 * rather than one per card: a list of two hundred rows costs one listener and
 * at most one style write a frame. It writes `--mx` and `--my` on the element
 * under the pointer; primitives.css draws the light from them. Touch is
 * skipped, since a finger has no hover to follow.
 */
export function installSpotlight(root: Document = document): () => void {
  let frame = 0;
  let latest: PointerEvent | null = null;

  const paint = () => {
    frame = 0;
    const event = latest;
    if (event === null || !(event.target instanceof Element)) return;
    const target = event.target.closest<HTMLElement>("[data-spotlight]");
    if (target === null) return;
    const point = spotlightPoint(event.clientX, event.clientY, target.getBoundingClientRect());
    target.style.setProperty("--mx", `${point.x}px`);
    target.style.setProperty("--my", `${point.y}px`);
  };

  const onMove = (event: PointerEvent) => {
    if (event.pointerType === "touch") return;
    latest = event;
    if (frame === 0) frame = window.requestAnimationFrame(paint);
  };

  root.addEventListener("pointermove", onMove, { passive: true });
  return () => {
    root.removeEventListener("pointermove", onMove);
    window.cancelAnimationFrame(frame);
  };
}
