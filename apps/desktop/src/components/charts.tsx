/**
 * What a chart in this app is made of, as data and arithmetic: the tones,
 * hatches and spans it draws, where an instant falls across its width, the
 * ticks of its axis, and how a length of time is written. Pure functions,
 * tested in charts.test.ts; the drawn parts that use them are in
 * chartParts.tsx.
 *
 * Only what the run timeline needs is here. Geometry is percentages of the
 * drawing's width, carried in SVG geometry attributes. Nothing is measured and
 * nothing is stored between renders, so the same data always draws the same
 * picture and a live chart redrawn every few seconds does not jitter.
 */

import { duration } from "../sdk/format";

/** The tones a span can take. Each but `neutral` is a modifier class in charts.css. */
export type ChartTone = "ok" | "danger" | "blocked" | "warn" | "info" | "neutral";

/**
 * The three hatches.
 *
 * - `refused`: a call the policy or a person refused.
 * - `skipped`: any other call that did not execute.
 * - `waiting`: time the run spent waiting for a decision.
 */
export type HatchKind = "refused" | "skipped" | "waiting";

/** The element ids of one chart's hatch patterns, made by `useHatches` (chartParts.tsx). */
export type Hatches = Readonly<Record<HatchKind, string>>;

/** The interval a chart draws, in milliseconds since the epoch. */
export interface Domain {
  start: number;
  end: number;
}

/**
 * The narrowest a span is drawn, as a percentage of the width.
 *
 * A two-millisecond read on an eleven-minute run is a sliver nobody could
 * click or see; widening it to this keeps every span a target, at the cost of
 * drawing the shortest calls slightly longer than they were. The table beside
 * the chart carries the true duration.
 */
export const MIN_SPAN_PERCENT = 0.4;

/** The height of a track's label line, in pixels. */
export const LABEL_HEIGHT = 16;

/** The height of a track's lane, matching `--chart-row` in tokens.css. */
export const LANE_HEIGHT = 22;

/** The space between tracks, a little more than `--chart-gap` to clear the labels. */
export const TRACK_GAP = 10;

/** The height of the axis beneath the tracks. */
export const AXIS_HEIGHT = 18;

/** One track's full height: its label line and its lane. */
export const TRACK_HEIGHT = LABEL_HEIGHT + LANE_HEIGHT;

/** Where an instant falls across the domain, as a percentage clamped to 0–100. */
export function position(at: number, domain: Domain): number {
  const length = domain.end - domain.start;
  if (!(length > 0) || !Number.isFinite(at)) return 0;
  return Math.min(100, Math.max(0, ((at - domain.start) / length) * 100));
}

/**
 * A span's left edge and width as percentages.
 *
 * At least {@link MIN_SPAN_PERCENT} wide, and pulled left if that minimum
 * would carry it past the end, so a call made in the run's last instant is
 * still drawn inside the chart rather than off its edge.
 */
export function spanGeometry(
  from: number,
  to: number,
  domain: Domain,
): { x: number; width: number } {
  const left = position(from, domain);
  const right = position(Math.max(from, to), domain);
  const width = Math.max(right - left, MIN_SPAN_PERCENT);
  return { x: Math.min(left, 100 - width), width };
}

/** A percentage as an SVG length. Three places is far below a pixel at any window size. */
export function percent(value: number): string {
  return `${Number(value.toFixed(3))}%`;
}

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;

/** Tick intervals an axis may use, each a length a person counts in. */
const STEPS = [
  SECOND,
  2 * SECOND,
  5 * SECOND,
  10 * SECOND,
  15 * SECOND,
  30 * SECOND,
  MINUTE,
  2 * MINUTE,
  5 * MINUTE,
  10 * MINUTE,
  15 * MINUTE,
  30 * MINUTE,
  HOUR,
  2 * HOUR,
  3 * HOUR,
  6 * HOUR,
  12 * HOUR,
  24 * HOUR,
];

/**
 * Tick offsets from the start of a duration, in milliseconds.
 *
 * The smallest interval from {@link STEPS} that gives at most `most` ticks
 * after zero, so a ninety-second run is marked every 15s and an hour-long one
 * every 10m. Ticks are offsets rather than clock times: "how far into the run"
 * is the question the axis answers, and the table carries the clock.
 */
export function ticks(length: number, most = 6): number[] {
  if (!(length > 0) || !Number.isFinite(length)) return [0];
  const step = STEPS.find((candidate) => length / candidate <= most) ?? Math.ceil(length / most);
  const out: number[] = [];
  for (let at = 0; at <= length; at += step) out.push(at);
  return out;
}

/** An offset as an axis label: `0`, `45s`, `2m`, `2m 30s`, `1h 15m`. */
export function offsetLabel(ms: number): string {
  if (ms <= 0) return "0";
  const seconds = Math.round(ms / SECOND);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    const rest = seconds % 60;
    return rest === 0 ? `${minutes}m` : `${minutes}m ${rest}s`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

/**
 * A span of time in the largest units that read naturally: `850ms`, `42s`,
 * `11m`, `3h 20m`, `2d 4h`.
 *
 * `duration` stops at minutes, which suits a tool call; a run left waiting on
 * an approval overnight would read as `840m`.
 */
export function elapsed(ms: number): string {
  if (!(ms >= 1_000)) return duration(Math.max(0, Math.round(ms)));
  const seconds = Math.round(ms / 1_000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 === 0 ? `${hours}h` : `${hours}h ${minutes % 60}m`;
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days}d` : `${days}d ${hours % 24}h`;
}

/** One span on a track. */
export interface TrackSpan {
  /** Unique on the track; handed back to `onSelect`. */
  id: string;
  /** Milliseconds since the epoch. */
  from: number;
  to: number;
  tone: ChartTone;
  hatch: HatchKind | null;
  /** What the span is, in words: its accessible name and its tooltip. */
  label: string;
}

/** A point on a track worth calling out, drawn as a dot with a label beside it. */
export interface TrackMarker {
  at: number;
  label: string;
}
