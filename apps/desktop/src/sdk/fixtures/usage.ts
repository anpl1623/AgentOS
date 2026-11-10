/** Tool usage, a day at a time, over any window the runtime would answer. */

import type { ToolUsageView } from "../../bindings/ToolUsageView";

import { minutesAgo, now } from "./clock";
import { settings } from "./settings";

/** How each day before this week compares with the week's average. */
const EARLIER_RHYTHM = [0.6, 0.4, 0.7, 0.5, 0.3, 0.8, 0.5];
const HISTORY_DAYS = 30;
/** The longest window the runtime looks back over, in days. */
const MAX_USAGE_DAYS = 3_650;

/**
 * A tool's calls on each of the last thirty days, oldest first.
 *
 * A tool last used before midnight made no call today, so if the fixtures load
 * early in the morning, today's calls are moved onto yesterday: the week's
 * total stays as the table says, and a one-day window agrees with the time of
 * last use beside it.
 */
function usageHistory(week: readonly number[], lastMinutesAgo: number | null): number[] {
  const average = week.reduce((total, calls) => total + calls, 0) / week.length;
  const earlier = Array.from({ length: HISTORY_DAYS - week.length }, (_, day) =>
    Math.round(average * EARLIER_RHYTHM[day % EARLIER_RHYTHM.length]!),
  );
  const history = [...earlier, ...week];
  const sinceMidnight = now.getHours() * 60 + now.getMinutes();
  if (lastMinutesAgo !== null && lastMinutesAgo >= sinceMidnight) {
    history[history.length - 2]! += history.at(-1)!;
    history[history.length - 1] = 0;
  }
  return history;
}

/**
 * Split `total` into whole numbers in proportion to `weights`, the remainder
 * going to the largest fractions first and, between equals, to the earlier.
 * The parts always sum to `total`, so a bucket scaled this way never claims
 * more calls than the window has.
 */
function apportion(weights: readonly number[], total: number): number[] {
  const whole = weights.reduce((sum, weight) => sum + weight, 0);
  if (whole === 0) return weights.map(() => 0);
  const exact = weights.map((weight) => (weight * total) / whole);
  const parts = exact.map(Math.floor);
  const order = exact
    .map((value, index) => ({ index, fraction: value - Math.floor(value) }))
    .sort((a, b) => b.fraction - a.fraction || a.index - b.index);
  let left = total - parts.reduce((sum, part) => sum + part, 0);
  for (const { index } of order) {
    if (left <= 0) break;
    parts[index]! += 1;
    left -= 1;
  }
  return parts;
}

/**
 * Tool usage over the default week. The sales agent's browser work accounts
 * for nearly all of it, and `browser.type` was refused several times, every
 * time on a run that had read the injected CRM record.
 *
 * The last column is the week day by day, oldest first, today last; it sums to
 * the calls. The weeks before it are quieter, in the same working rhythm, so
 * there are thirty days of history behind the month's window.
 */
const usageWeek = (
  [
    // tool, calls, executed, denied, approval_denied, failed, under_taint,
    // approvals, total ms, minutes since last use, the week by day
    ["browser.navigate", 59, 58, 0, 1, 2, 22, 4, 8_930, 9, [6, 9, 7, 11, 8, 10, 8]],
    ["browser.extract", 44, 44, 0, 0, 0, 30, 0, 312, 8, [4, 7, 5, 8, 6, 8, 6]],
    ["browser.type", 8, 2, 5, 1, 0, 7, 3, 1_240, 1, [0, 1, 0, 2, 1, 1, 3]],
    ["filesystem.write", 6, 6, 0, 0, 0, 0, 0, 41, 176, [1, 0, 1, 1, 0, 2, 1]],
    ["filesystem.read", 4, 1, 3, 0, 0, 3, 0, 3, 6, [0, 1, 0, 0, 1, 0, 2]],
    ["terminal.exec", 1, 0, 1, 0, 0, 1, 0, 0, 5, [0, 0, 0, 0, 0, 0, 1]],
    ["filesystem.delete", 0, 0, 0, 0, 0, 0, 0, 0, null, [0, 0, 0, 0, 0, 0, 0]],
  ] as const
).map(
  ([tool, calls, executed, denied, approvalDenied, failed, underTaint, approvals, ms, last, week]) => {
    const catalogue = settings.tools.find((each) => each.name === tool);
    return {
      tool,
      calls,
      executed,
      denied,
      approval_denied: approvalDenied,
      failed,
      under_taint: underTaint,
      approvals,
      total_duration_ms: ms,
      last_used_at: last === null ? null : minutesAgo(last),
      risk: catalogue?.risk ?? null,
      returns_untrusted_data: catalogue?.returns_untrusted_data ?? false,
      history: usageHistory(week, last),
    };
  },
);

/**
 * Tool usage over the `days` days ending today, as the runtime answers it:
 * seven unless given, clamped to between one day and ten years, with `daily`
 * as long as the window and summing to `calls`, and busiest first, equal
 * counts in name order.
 *
 * The week is the table above, exactly. Another window counts its own days
 * from each tool's history and splits them among the outcomes in the week's
 * proportions, so today, the week and the month are three answers that agree
 * with each other rather than one answer given three times.
 */
export function toolUsageOver(asked: unknown): ToolUsageView[] {
  const requested = typeof asked === "number" && Number.isFinite(asked) ? Math.trunc(asked) : 7;
  const days = Math.min(Math.max(requested, 1), MAX_USAGE_DAYS);
  return usageWeek
    .map(({ history, ...week }) => {
      const recent = history.slice(-days);
      const daily = [...Array<number>(days - recent.length).fill(0), ...recent];
      const calls = daily.reduce((total, count) => total + count, 0);
      const share = week.calls === 0 ? 0 : calls / week.calls;
      const otherwise = week.calls - week.executed - week.denied - week.approval_denied;
      const [executed = 0, denied = 0, approvalDenied = 0] = apportion(
        [week.executed, week.denied, week.approval_denied, otherwise],
        calls,
      );
      const scaled = (count: number, most: number) => Math.min(Math.round(count * share), most);
      return {
        ...week,
        calls,
        executed,
        denied,
        approval_denied: approvalDenied,
        failed: scaled(week.failed, executed),
        under_taint: scaled(week.under_taint, calls),
        approvals: scaled(week.approvals, calls),
        total_duration_ms: Math.round(week.total_duration_ms * share),
        last_used_at: calls === 0 ? null : week.last_used_at,
        daily,
      } satisfies ToolUsageView;
    })
    .sort((a, b) => b.calls - a.calls || (a.tool < b.tool ? -1 : a.tool > b.tool ? 1 : 0));
}
