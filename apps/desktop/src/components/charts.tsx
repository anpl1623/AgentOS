/**
 * The parts a chart in this app is made of, and nothing more.
 *
 * Only what the run timeline needs is here: a figure, horizontal tracks of
 * spans over a time axis, a legend and the hatches. A chart is the second way
 * of saying something the screen also says in text, so {@link Figure} takes
 * its text equivalent as a required prop; a chart written without one does not
 * compile.
 *
 * Geometry is percentages of the drawing's width, carried in SVG geometry
 * attributes. Nothing is measured and nothing is stored between renders, so
 * the same data always draws the same picture and a live chart redrawn every
 * few seconds does not jitter. Colour comes from the classes in charts.css,
 * which take it from the status ramps; every tone that means something also
 * has a shape, the hatch, so the chart still reads in greyscale.
 */

import { type KeyboardEvent, type ReactNode, useId } from "react";

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

/** The element ids of one chart's hatch patterns, made by {@link useHatches}. */
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

/** The class a span is drawn with. */
function spanClass(tone: ChartTone): string {
  return tone === "neutral" ? "chart-span" : `chart-span ${tone}`;
}

/**
 * Element ids for one chart's hatches.
 *
 * Made per chart rather than fixed, so two charts on one page never define
 * the same id; the patterns are mounted once per chart, by {@link HatchDefs}.
 */
export function useHatches(): Hatches {
  const base = useId();
  return {
    refused: `${base}-refused`,
    skipped: `${base}-skipped`,
    waiting: `${base}-waiting`,
  };
}

/**
 * The hatch patterns, mounted once inside a chart's `<svg>`.
 *
 * Three directions of line, so a refused call, a call that otherwise did not
 * run, and time spent waiting are told apart by shape as well as by tone.
 */
export function HatchDefs({ hatches }: { hatches: Hatches }) {
  const pattern = (id: string, angle: number, gap: number) => (
    <pattern
      id={id}
      width={gap}
      height={gap}
      patternUnits="userSpaceOnUse"
      patternTransform={`rotate(${angle})`}
    >
      <line className="chart-hatch" x1="0" y1="0" x2="0" y2={gap} />
    </pattern>
  );
  return (
    <defs>
      {pattern(hatches.refused, 45, 6)}
      {pattern(hatches.skipped, 90, 5)}
      {pattern(hatches.waiting, -45, 6)}
    </defs>
  );
}

/**
 * A chart with its caption and its text equivalent.
 *
 * `table` is required: the facts the drawing shows, as text a screen reader
 * can walk and a person can copy, behind a disclosure under the caption.
 */
export function Figure({
  title,
  caption,
  table,
  children,
}: {
  /** What the chart is, for the disclosure that holds its table. */
  title: string;
  caption: ReactNode;
  table: ReactNode;
  children: ReactNode;
}) {
  return (
    <figure className="figure">
      {children}
      <figcaption className="figure-caption">{caption}</figcaption>
      <details className="figure-data">
        <summary>{title} as a table</summary>
        {table}
      </details>
    </figure>
  );
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

/**
 * One labelled lane of spans.
 *
 * Every span is a keyboard stop with a name, so the chart can be read and
 * used without a pointer: Enter or Space selects it as a click does. A span is
 * drawn as a group whose class carries the tone, and its rectangles inherit
 * the fill and stroke, which is what lets the focus ring follow the shape.
 */
export function Track({
  label,
  y,
  domain,
  spans,
  hatches,
  markers = [],
  onSelect,
}: {
  label: string;
  /** The top of the track's label line. */
  y: number;
  domain: Domain;
  spans: readonly TrackSpan[];
  hatches: Hatches;
  markers?: readonly TrackMarker[];
  onSelect: (id: string) => void;
}) {
  const laneY = y + LABEL_HEIGHT;
  const keyDown = (event: KeyboardEvent, id: string) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onSelect(id);
  };
  return (
    <g>
      <text className="chart-label" x="0" y={y + LABEL_HEIGHT - 4}>
        {label}
      </text>
      <line
        className="chart-grid"
        x1="0"
        x2="100%"
        y1={laneY + LANE_HEIGHT}
        y2={laneY + LANE_HEIGHT}
      />
      {spans.map((span) => {
        const { x, width } = spanGeometry(span.from, span.to, domain);
        return (
          <g
            key={span.id}
            className={spanClass(span.tone)}
            tabIndex={0}
            role="button"
            aria-label={span.label}
            onClick={() => onSelect(span.id)}
            onKeyDown={(event) => keyDown(event, span.id)}
          >
            <title>{span.label}</title>
            <rect x={percent(x)} y={laneY} width={percent(width)} height={LANE_HEIGHT} rx="2" />
            {span.hatch !== null ? (
              <rect
                x={percent(x)}
                y={laneY}
                width={percent(width)}
                height={LANE_HEIGHT}
                rx="2"
                fill={`url(#${hatches[span.hatch]})`}
                stroke="none"
                pointerEvents="none"
              />
            ) : null}
          </g>
        );
      })}
      {markers.map((marker) => {
        const x = position(marker.at, domain);
        // The label sits on the track's label line, after the dot, or before
        // it near the right edge; never over the track's own name.
        const textX = x > 70 ? x - 1 : Math.max(x + 1, 9);
        return (
          <g key={`${marker.at}-${marker.label}`} aria-hidden="true">
            <circle className="chart-taint" cx={percent(x)} cy={laneY} r="4" />
            <text
              className="chart-label"
              x={percent(textX)}
              y={y + LABEL_HEIGHT - 4}
              textAnchor={x > 70 ? "end" : "start"}
            >
              {marker.label}
            </text>
          </g>
        );
      })}
    </g>
  );
}

/**
 * The time axis beneath the tracks, with a grid line up through them at each
 * tick. Drawn first so the spans paint over its lines.
 */
export function Axis({ y, domain }: { y: number; domain: Domain }) {
  const length = domain.end - domain.start;
  const marks = ticks(length);
  return (
    <g aria-hidden="true">
      {marks.map((offset, index) => {
        const x = position(domain.start + offset, domain);
        const anchor = index === 0 ? "start" : x > 97 ? "end" : "middle";
        return (
          <g key={offset}>
            <line className="chart-grid" x1={percent(x)} x2={percent(x)} y1="0" y2={y} />
            <text className="chart-axis" x={percent(x)} y={y + AXIS_HEIGHT - 4} textAnchor={anchor}>
              {offsetLabel(offset)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

/** One entry of a {@link Legend}. */
export interface LegendItem {
  label: string;
  tone: ChartTone;
  hatch?: HatchKind | undefined;
  /** Drawn as the track marker's dot rather than as a span swatch. */
  marker?: boolean | undefined;
}

/**
 * What each tone and hatch means.
 *
 * The swatches are small SVGs so a hatched swatch shows the same pattern the
 * spans do; they are decoration beside words that say the same thing.
 */
export function Legend({ items, hatches }: { items: readonly LegendItem[]; hatches: Hatches }) {
  return (
    <div className="chart-legend">
      {items.map((item) => (
        <span key={item.label} className="chart-legend-item">
          {item.marker ? (
            <svg className="chart-swatch" aria-hidden="true">
              <circle className="chart-taint" cx="50%" cy="50%" r="4" />
            </svg>
          ) : (
            <svg
              className={item.tone === "neutral" ? "chart-swatch" : `chart-swatch ${item.tone}`}
              aria-hidden="true"
            >
              {item.hatch ? (
                <rect width="100%" height="100%" fill={`url(#${hatches[item.hatch]})`} />
              ) : null}
            </svg>
          )}
          {item.label}
        </span>
      ))}
    </div>
  );
}
