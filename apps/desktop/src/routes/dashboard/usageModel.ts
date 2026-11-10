/**
 * The tool-usage series: each tool's calls day by day, the day's total across
 * every tool, and what today can honestly be compared with.
 *
 * The runtime's `daily` is one count per local calendar day, oldest first and
 * today last, as long as the window asked for. Today is a day still under
 * way, so it is never set against the whole days before it as if it were one
 * of them. Pure, so the words and the line a tile draws are fixed by a test.
 */

import type { ToolUsageView } from "../../bindings/ToolUsageView";
import type { StatDelta } from "../../components/StatTile";

/**
 * One tool's calls per day, or `null` when the report has no series for it.
 *
 * A runtime older than the window sends no `daily` at all, and a count that
 * is not a number is not drawn as one: such a series is left out rather than
 * drawn as a line of zeros that says the tool was idle.
 */
export function toolSeries(view: ToolUsageView): readonly number[] | null {
  const daily: unknown = view.daily;
  if (!Array.isArray(daily) || daily.length === 0) return null;
  return daily.every((count) => typeof count === "number" && Number.isFinite(count) && count >= 0)
    ? (daily as number[])
    : null;
}

/**
 * Every tool's calls added up day by day, oldest first and today last, or
 * `null` when no tool in the report carries a series.
 *
 * Each series ends on today, so a shorter one is lined up by its last day
 * rather than its first: a day it does not reach adds nothing, and today is
 * never added to a day before it.
 */
export function dailyCalls(usage: readonly ToolUsageView[]): number[] | null {
  const series = usage
    .map(toolSeries)
    .filter((each): each is readonly number[] => each !== null);
  if (series.length === 0) return null;
  const days = Math.max(...series.map((each) => each.length));
  const totals = Array<number>(days).fill(0);
  for (const each of series) {
    const offset = days - each.length;
    each.forEach((count, index) => {
      totals[offset + index] = (totals[offset + index] ?? 0) + count;
    });
  }
  return totals;
}

/**
 * What a per-day line is, for its accessible name: the measure, its unit and
 * its window, and that the last point is a day not yet over. The sparkline
 * adds the numbers.
 */
export function perDayLabel(measure: string, days: number): string {
  if (days <= 1) return `${measure} today so far`;
  return `${measure} per day over the last ${days} days, ending with today so far`;
}

/**
 * The words beneath a per-day line on a tile: what it counts and how far back
 * it reaches, since the line itself carries no axis.
 */
export function perDayCaption(days: number): string {
  return days <= 1 ? "today so far" : `per day, last ${days} days`;
}

/**
 * Today's calls against the whole days before it, as a tile's delta, or
 * `null` when there is nothing true to say.
 *
 * Today is still under way, so it can only be shown to be ahead: a morning
 * below the average says nothing yet about the evening, and an arrow down or
 * a level arrow would claim it does. The delta appears once today has passed
 * the earlier days' average by more than the 15% a day wanders on its own;
 * otherwise the line beside it shows the days and says nothing it cannot
 * know. Neutral, because more tool calls is neither good nor bad news.
 *
 * `series` is the window's daily totals with today last, from
 * {@link dailyCalls}; the days before today are everything but the last. How
 * far back they reach is the line's caption to say, not the delta's.
 */
export function callsDelta(today: number, series: readonly number[] | null): StatDelta | null {
  if (series === null || series.length < 2 || !Number.isFinite(today)) return null;
  const before = series.slice(0, -1);
  const total = before.reduce((sum, count) => sum + count, 0);
  if (!(total > 0)) return null;
  const average = total / before.length;
  if (!(today > average * 1.15)) return null;
  const shown = average < 10 ? String(Math.round(average * 10) / 10) : String(Math.round(average));
  return { text: `above ${shown}/day before today`, direction: "up", sentiment: "neutral" };
}
