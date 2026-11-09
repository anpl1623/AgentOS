/**
 * What the top of the schedules screen says: the figures above the list and
 * the scheduler's state, apart from how they are drawn.
 *
 * Counted the way the cards beneath read (`schedulesModel.ts`), as pure
 * functions over an explicit `now`, so a figure above the list never disagrees
 * with the cards and a test fails when either changes.
 */

import type { ScheduleView } from "../../bindings/ScheduleView";
import type { SchedulerView } from "../../bindings/SchedulerView";
import { countdown, relative, spanOfSeconds, timeOf } from "./timeModel";

// ---------------------------------------------------------------------------
// The figures
// ---------------------------------------------------------------------------

/** The figures above the list. */
export interface ScheduleSummary {
  active: number;
  paused: number;
  /** Active schedules whose moment has passed. */
  overdue: number;
  /** The active schedule that fires soonest, and how long until it does. */
  soonest: { name: string; figure: string } | null;
}

/**
 * Count the list the way its cards read, so a figure above the list never
 * disagrees with the cards beneath it.
 */
export function summarise(schedules: readonly ScheduleView[], now: number): ScheduleSummary {
  let active = 0;
  let paused = 0;
  let overdue = 0;
  let soonest: { name: string; at: number } | null = null;
  for (const schedule of schedules) {
    if (schedule.status === "paused") paused += 1;
    if (schedule.status !== "active") continue;
    active += 1;
    const at = timeOf(schedule.next_run_at);
    if (!Number.isFinite(at)) continue;
    if (at <= now) overdue += 1;
    else if (soonest === null || at < soonest.at) soonest = { name: schedule.name, at };
  }
  return {
    active,
    paused,
    overdue,
    soonest: soonest ? { name: soonest.name, figure: countdown(soonest.at - now) } : null,
  };
}

/**
 * The line under each count, saying what the figure means for what fires.
 *
 * Active schedules are only "firing" while the scheduler ticks; with it off
 * they are held, and the line says so rather than letting the count read as
 * work in progress. An unknown scheduler state claims neither.
 */
export function countHints(
  summary: ScheduleSummary,
  running: boolean | null,
): { active: string; paused: string; overdue: string } {
  let active: string;
  if (summary.active === 0) active = "none set to fire";
  else if (running === true) active = "firing on their cadence";
  else if (running === false) active = "held while the scheduler is off";
  else active = "set to fire";
  let overdue: string;
  if (summary.overdue === 0) overdue = "nothing past due";
  else if (running === true) overdue = "each fires on the next check";
  else overdue = "each fires once when it starts";
  return {
    active,
    paused: summary.paused === 0 ? "none held back" : "kept, not firing",
    overdue,
  };
}

/**
 * The "Next firing" figure and the line beneath it.
 *
 * Names the schedule that fires soonest, and says when the scheduler is off: a
 * countdown to a moment nothing will act on must not read as a promise.
 */
export function soonestTile(
  summary: ScheduleSummary,
  running: boolean | null,
): { value: string; hint: string } {
  if (summary.soonest === null) return { value: "none", hint: "nothing active is waiting" };
  const { name, figure } = summary.soonest;
  return { value: figure, hint: running === false ? `${name} · scheduler off` : name };
}

// ---------------------------------------------------------------------------
// The scheduler
// ---------------------------------------------------------------------------

/**
 * What a stopped scheduler's overdue schedules will do when it starts, or
 * `null` when none are overdue. Each fires once, not once for every
 * occurrence it missed; an operator expecting a burst, or nothing, is wrong.
 */
export function overdueNote(count: number): string | null {
  if (count <= 0) return null;
  return count === 1
    ? "One schedule is overdue; it fires once when the scheduler starts, not once for each occurrence it missed."
    : `${count} schedules are overdue; each fires once when the scheduler starts, not once for each occurrence it missed.`;
}

/** The short facts beside a running scheduler: how often, how many, since when. */
export function schedulerFacts(status: SchedulerView, now: number): string[] {
  const facts = [
    `Checks every ${spanOfSeconds(status.tick_seconds)}`,
    status.max_concurrent_runs === 1
      ? "One run at a time"
      : `Up to ${status.max_concurrent_runs} runs at once`,
  ];
  if (status.started_at && Number.isFinite(timeOf(status.started_at))) {
    const since = relative(status.started_at, now);
    facts.push(since === "now" ? "Started just now" : `Started ${since}`);
  }
  return facts;
}
