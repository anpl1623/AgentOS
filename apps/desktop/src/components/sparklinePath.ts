/**
 * A sparkline's geometry and its words, as pure functions of the numbers.
 *
 * The drawing is in a fixed coordinate box that the SVG stretches to whatever
 * box it is given, so a tile decides the line's size and this module never
 * needs to know it.
 */

/** The coordinate box every sparkline is drawn in. */
export const SPARK_BOX = { width: 100, height: 32, inset: 2 } as const;

export interface SparkGeometry {
  /** The line through the points; empty when there is nothing to draw. */
  line: string;
  /** The line closed down to the baseline, for the fill beneath it. */
  area: string;
}

/** A coordinate written to two places, without a trailing `.00`. */
function coordinate(value: number): string {
  return String(Math.round(value * 100) / 100);
}

/**
 * The paths for `values`.
 *
 * Counts are drawn from zero, not from their own minimum: a series that moved
 * from 100 to 104 is nearly flat, and drawing it as a cliff would claim a
 * change that is not there. A series that dips below zero keeps its floor.
 * Values that are not finite numbers are skipped rather than drawn as zero.
 */
export function sparklineGeometry(
  values: readonly number[],
  box: { width: number; height: number; inset: number } = SPARK_BOX,
): SparkGeometry {
  const points = values.filter((value) => Number.isFinite(value));
  if (points.length === 0) return { line: "", area: "" };

  const floor = Math.min(0, ...points);
  const ceiling = Math.max(...points);
  const span = ceiling - floor;
  const innerWidth = box.width - box.inset * 2;
  const innerHeight = box.height - box.inset * 2;
  const baseline = box.height - box.inset;

  // A single point, or a series that never leaves zero, is a level line.
  const series = points.length === 1 ? [points[0] ?? 0, points[0] ?? 0] : points;
  const step = innerWidth / (series.length - 1);
  const xy = series.map((value, index) => {
    const x = box.inset + step * index;
    const y = span === 0 ? baseline : box.inset + (1 - (value - floor) / span) * innerHeight;
    return [coordinate(x), coordinate(y)] as const;
  });

  const line = xy.map(([x, y], index) => `${index === 0 ? "M" : "L"}${x} ${y}`).join(" ");
  const first = xy[0]?.[0] ?? "0";
  const last = xy[xy.length - 1]?.[0] ?? "0";
  const area = `${line} L${last} ${coordinate(baseline)} L${first} ${coordinate(baseline)} Z`;
  return { line, area };
}

/**
 * The series in words, for the sparkline's accessible name: where it started,
 * where it ended, and its range. A picture of a trend says nothing to a screen
 * reader unless it is told.
 */
export function describeSeries(values: readonly number[]): string {
  const points = values.filter((value) => Number.isFinite(value));
  const first = points[0];
  const last = points[points.length - 1];
  if (first === undefined || last === undefined) return "no data";
  if (points.length === 1) return `one value, ${first}`;
  const low = Math.min(...points);
  const high = Math.max(...points);
  return `${points.length} values, from ${first} to ${last}; lowest ${low}, highest ${high}`;
}
