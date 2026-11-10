/**
 * The parts a chart in this app is drawn from: a figure, horizontal tracks of
 * spans over a time axis, a legend and the hatches. The arithmetic they share
 * is in charts.tsx.
 *
 * A chart is the second way of saying something the screen also says in
 * text, so {@link Figure} takes its text equivalent as a required prop; a
 * chart written without one does not compile. Colour comes from the classes
 * in charts.css, which take it from the status ramps; every tone that means
 * something also has a shape, the hatch, so the chart still reads in
 * greyscale.
 */

import { type KeyboardEvent, type ReactNode, useId } from "react";

import {
  AXIS_HEIGHT,
  type ChartTone,
  type Domain,
  type HatchKind,
  type Hatches,
  LABEL_HEIGHT,
  LANE_HEIGHT,
  type TrackMarker,
  type TrackSpan,
  offsetLabel,
  percent,
  position,
  spanGeometry,
  ticks,
} from "./charts";

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
      {/* The lane is a shallow well the spans sit in; translucent, so the
          axis's grid lines still show through it. */}
      <rect className="chart-well" x="0" y={laneY} width="100%" height={LANE_HEIGHT} rx="4" />
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
            <rect x={percent(x)} y={laneY} width={percent(width)} height={LANE_HEIGHT} rx="3" />
            {span.hatch !== null ? (
              <rect
                x={percent(x)}
                y={laneY}
                width={percent(width)}
                height={LANE_HEIGHT}
                rx="3"
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
