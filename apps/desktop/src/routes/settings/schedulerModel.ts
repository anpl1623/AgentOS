/**
 * The scheduler panel: the pacing fields as typed, and what the panel claims
 * about the work the scheduler would find.
 */

import type { SchedulerView } from "../../bindings/SchedulerView";
import { asSentence, relative } from "./settingsModel";

/**
 * A count typed into a number field, or `null` if it is not a whole number.
 *
 * Only the shape is checked. Whether five seconds is too short a tick is the
 * runtime's to say, and its refusal is shown as written; a second copy of
 * that rule here would one day disagree with the first.
 */
export function parseCount(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) ? value : null;
}

/** The pacing fields as typed, read against the scheduler's own. */
export interface Pacing {
  tick: number | null;
  max: number | null;
  /** Both fields hold whole numbers, so the pacing can be sent. */
  valid: boolean;
  /** The fields say something other than the scheduler's current pacing. */
  changed: boolean;
}

/** Read the pacing fields. Text that is not a whole number counts as changed. */
export function readPacing(view: SchedulerView, tickText: string, maxText: string): Pacing {
  const tick = parseCount(tickText);
  const max = parseCount(maxText);
  return {
    tick,
    max,
    valid: tick !== null && max !== null,
    changed: tick !== view.tick_seconds || max !== view.max_concurrent_runs,
  };
}

/**
 * One sentence on what the scheduler is doing.
 *
 * Off says what is kept and what does not happen, because "off" alone leaves
 * an operator to guess whether turning it off threw their schedules away.
 */
export function schedulerSummary(view: SchedulerView, now: number): string {
  if (!view.running) {
    return "Off. Schedules are kept, but nothing fires and no queued task starts.";
  }
  const since = view.started_at ? `, started ${relative(view.started_at, now)}` : "";
  const runs = view.max_concurrent_runs === 1 ? "one run" : `${view.max_concurrent_runs} runs`;
  return (
    `Running${since}. It looks for due work every ${view.tick_seconds}s, ` +
    `with up to ${runs} at once.`
  );
}

/** One line of the scheduler's counts. */
export interface SchedulerFact {
  label: string;
  value: string;
  /** Something an operator should look at, drawn in the attention tone. */
  attention: boolean;
}

/**
 * When the soonest schedule fires.
 *
 * A moment already passed is "due" rather than "in": with the scheduler
 * running it fires on the next tick, and with it off it waits, which the
 * summary beside it already says.
 */
export function nextFiring(view: SchedulerView, now: number): string {
  if (view.next_fire_at === null) return "nothing scheduled";
  const at = Date.parse(view.next_fire_at);
  if (at > now) return relative(view.next_fire_at, now);
  return view.running ? "due now" : `due ${relative(view.next_fire_at, now)}`;
}

/**
 * The work the scheduler would find, as label and value pairs.
 *
 * Overdue schedules and tasks that can never start are flagged only when
 * there are some. A stopped scheduler with schedules already due is drawn
 * as needing attention, which is the state these counts exist to show.
 */
export function schedulerFacts(view: SchedulerView, now: number): SchedulerFact[] {
  return [
    { label: "Active schedules", value: String(view.active_schedules), attention: false },
    { label: "Next firing", value: nextFiring(view, now), attention: false },
    { label: "Overdue", value: String(view.overdue), attention: view.overdue > 0 },
    { label: "Ready to start", value: String(view.runnable_tasks), attention: false },
    {
      label: "Can never start",
      value: String(view.unreachable_tasks),
      attention: view.unreachable_tasks > 0,
    },
  ];
}

/**
 * What went wrong with the scheduler, each said once.
 *
 * A switch refused because another process holds the lease is reported twice
 * by the runtime: as the call's refusal and as the status view's `error`.
 * They are the same words, so the second is dropped. A refusal the status
 * does not repeat, such as pacing it would not accept, is still shown, as is
 * a stop the scheduler came to on its own while nobody pressed anything.
 */
export function schedulerNotices(view: SchedulerView | null, refusal: string | null): string[] {
  const notices: string[] = [];
  if (refusal !== null) notices.push(asSentence(refusal));
  const stopped = view?.running === false ? view.error : null;
  if (stopped && !notices.includes(asSentence(stopped))) notices.push(asSentence(stopped));
  return notices;
}
