/**
 * What the schedules screen says about the schedules it lists and the
 * scheduler that fires them, apart from how it draws it.
 *
 * What each schedule's card claims about when it fires, the order of the list,
 * and the delete question live here as pure functions over an explicit `now`,
 * so a test fails when one of them changes. The figures above the list and the
 * scheduler's state are in `overviewModel.ts`, the form's decisions in
 * `formModel.ts`, the words for a cadence in `cadenceModel.ts`, and the clock
 * arithmetic in `timeModel.ts`.
 */

import type { CadenceView } from "../../bindings/CadenceView";
import type { ScheduleView } from "../../bindings/ScheduleView";
import type { VerdictTone } from "../../components/status";
import { cadenceWords } from "./cadenceModel";
import {
  countdown,
  lengthOf,
  localMoment,
  relative,
  spanOfSeconds,
  timeOf,
} from "./timeModel";

// ---------------------------------------------------------------------------
// One schedule
// ---------------------------------------------------------------------------

/**
 * Schedule statuses.
 *
 * Only `active` is drawn in a colour, and it is the quiet `ok`: an active
 * schedule is one that would fire, not one that is firing. A paused or finished
 * schedule is neither good nor bad news.
 */
const SCHEDULE_TONES = {
  active: "ok",
  paused: "neutral",
  finished: "neutral",
} as const satisfies Record<string, VerdictTone>;

/** The chip tone for a schedule status; neutral for one this build does not know. */
export function scheduleTone(status: string): VerdictTone {
  return Object.hasOwn(SCHEDULE_TONES, status)
    ? SCHEDULE_TONES[status as keyof typeof SCHEDULE_TONES]
    : "neutral";
}

/** The runtime's description of a cadence, with an interval also said in hours. */
export function cadenceLabel(cadence: CadenceView): string {
  if (cadence.kind === "every" && cadence.seconds !== null) {
    const span = spanOfSeconds(cadence.seconds);
    if (span !== `${cadence.seconds}s`) return `${cadence.description} (${span})`;
  }
  return cadence.description;
}

/** What one schedule's row says, apart from its name, objective and agent. */
export interface ScheduleRowView {
  /** The chip's tone and text. */
  tone: VerdictTone;
  label: string;
  /** When it next fires, in words. */
  next: string;
  /** The absolute moment behind `next`, for the `title`; `null` when there is none. */
  nextAt: string | null;
  /** An active schedule whose moment has passed without it firing. */
  overdue: boolean;
  /** When it last fired, in words. */
  last: string;
  /** The one state change the row offers: pause, resume, or none. */
  action: "pause" | "resume" | null;
  /**
   * What resuming will do, said beside Resume. Resuming never replays a
   * backlog, and an operator who assumes it will — or assumes it fires at once
   * — is wrong in whichever direction hurts.
   */
  resumeNote: string | null;
}

/** Capitalise a wire value for a chip. */
function sentence(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

/**
 * Everything a schedule's row claims about when it fires.
 *
 * Mirrors the runtime's rules rather than restating the stored fields:
 *
 * - An active schedule whose moment has passed is overdue. It fires once on
 *   the scheduler's next tick and continues from that firing; the occurrences
 *   it missed are not fired one by one.
 * - A paused schedule keeps its stored moment, but that moment is only what
 *   resuming would keep if it is still ahead. Resuming otherwise moves to the
 *   next occurrence after now, and a one-shot whose moment has passed is
 *   finished rather than fired.
 * - A finished schedule will not fire again; it offers neither pause nor
 *   resume, only deletion.
 */
export function scheduleRow(schedule: ScheduleView, now: number): ScheduleRowView {
  const nextAt = schedule.next_run_at;
  const nextTime = timeOf(nextAt);
  const ahead = Number.isFinite(nextTime) && nextTime > now;
  const last = schedule.last_run_at
    ? `last fired ${relative(schedule.last_run_at, now)}`
    : "never fired";
  const base = {
    tone: scheduleTone(schedule.status),
    label: sentence(schedule.status.replace(/_/g, " ")),
    nextAt,
    last,
  };

  switch (schedule.status) {
    case "active": {
      if (nextAt === null || !Number.isFinite(nextTime)) {
        return {
          ...base,
          next: "no next firing",
          overdue: false,
          action: "pause",
          resumeNote: null,
        };
      }
      return {
        ...base,
        next: ahead ? `next ${relative(nextAt, now)}` : `overdue by ${lengthOf(now - nextTime)}`,
        overdue: !ahead,
        action: "pause",
        resumeNote: null,
      };
    }
    case "paused": {
      const once = schedule.cadence.kind === "once";
      let next: string;
      let resumeNote: string;
      if (ahead && nextAt !== null) {
        next = `would fire ${relative(nextAt, now)}`;
        resumeNote = "Resuming keeps that firing; nothing was missed.";
      } else if (once) {
        next = "its moment passed while paused";
        resumeNote = "Its moment has passed, so resuming finishes it rather than firing it.";
      } else {
        next = "not firing";
        resumeNote =
          "Resuming picks up at the next occurrence from now; firings missed while paused " +
          "are not made up.";
      }
      return { ...base, next, overdue: false, action: "resume", resumeNote };
    }
    case "finished":
      return {
        ...base,
        next: "will not fire again",
        overdue: false,
        action: null,
        resumeNote: null,
      };
    default:
      // A status this build does not know. Offer nothing that would change
      // it: pausing or resuming something whose meaning is unknown is a guess.
      return {
        ...base,
        next: nextAt ? `next ${relative(nextAt, now)}` : "no next firing",
        overdue: false,
        action: null,
        resumeNote: null,
      };
  }
}

// ---------------------------------------------------------------------------
// One schedule's card
// ---------------------------------------------------------------------------

/** The "next firing" block of a card. */
export interface WhenView {
  /** The block's eyebrow. */
  label: string;
  /** A countdown such as `22h 28m`, or `null` when there is nothing to count to. */
  figure: string | null;
  /** The moment, as `Tue 10 Nov 09:00` or `Was due Mon 9 Nov 16:00`; `null` when there is none. */
  at: string | null;
  /** A sentence under the moment, or in the figure's place; `null` when the moment says it all. */
  caption: string | null;
  /** `warn` for an overdue schedule; `quiet` for one that is not counting down. */
  tone: "normal" | "warn" | "quiet";
  /** How far through its wait the schedule is, 0 to 1; `null` when it is not waiting. */
  progress: number | null;
}

/** Everything a schedule's card draws, beyond its name, objective and agent. */
export interface ScheduleCardView {
  row: ScheduleRowView;
  /** The cadence in words, or the runtime's own description when there are none. */
  words: string;
  /** The runtime's description, beneath the words, when the words are not it. */
  exact: string | null;
  when: WhenView;
  /** When it last fired, relative: `1h ago`, or `never`. */
  lastFired: string;
  /** The same moment in full, for its `title`. */
  lastFiredAt: string | null;
}

/**
 * How far through its wait a schedule is: from the moment it last fired (or
 * was created, if it never has) to the moment it next fires.
 *
 * `null` when either end cannot be read or the wait has no length, so a bar
 * is not drawn from nonsense.
 */
export function cycleProgress(schedule: ScheduleView, now: number): number | null {
  const start = timeOf(schedule.last_run_at ?? schedule.created_at);
  const end = timeOf(schedule.next_run_at);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) return null;
  return Math.min(1, Math.max(0, (now - start) / (end - start)));
}

/**
 * The card for one schedule.
 *
 * `running` is whether the scheduler in this window is ticking, or `null`
 * when that could not be read. An active schedule's countdown is still shown
 * while the scheduler is off, because that is when it is due, but the card
 * says that being due is not the same as firing.
 */
export function scheduleCard(
  schedule: ScheduleView,
  now: number,
  running: boolean | null,
): ScheduleCardView {
  const row = scheduleRow(schedule, now);
  const words = cadenceWords(schedule.cadence) ?? cadenceLabel(schedule.cadence);
  const exact =
    schedule.cadence.kind === "once" || words === schedule.cadence.description
      ? null
      : schedule.cadence.description;
  const nextTime = timeOf(schedule.next_run_at);
  const at = Number.isFinite(nextTime) ? localMoment(nextTime) : null;
  const lastTime = timeOf(schedule.last_run_at);

  let when: WhenView;
  if (schedule.status === "active" && Number.isFinite(nextTime)) {
    if (row.overdue) {
      when = {
        label: "Overdue by",
        figure: countdown(now - nextTime),
        at: `Was due ${at}`,
        caption:
          running === true
            ? "It fires on the scheduler's next check."
            : "It fires once when the scheduler starts.",
        tone: "warn",
        progress: null,
      };
    } else {
      when = {
        label: "Next firing",
        figure: countdown(nextTime - now),
        at,
        caption: running === false ? "Due then, but the scheduler is off." : null,
        tone: "normal",
        progress: cycleProgress(schedule, now),
      };
    }
  } else {
    // Paused, finished, or active with no moment: nothing is counting down,
    // so the row's own words stand in for the figure.
    const keptSlot = schedule.status === "paused" && Number.isFinite(nextTime) && nextTime > now;
    when = {
      label: "Next firing",
      figure: null,
      at: keptSlot ? at : null,
      caption: sentence(row.next),
      tone: "quiet",
      progress: null,
    };
  }

  return {
    row,
    words,
    exact,
    when,
    lastFired: schedule.last_run_at ? relative(schedule.last_run_at, now) : "never",
    lastFiredAt: Number.isFinite(lastTime) ? localMoment(lastTime) : null,
  };
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/**
 * The element id of a schedule's card. The screen looks for focus inside it
 * after a delete, to move focus on only if it would otherwise be lost.
 */
export function scheduleCardId(id: string): string {
  return `schedule-card-${id}`;
}

/**
 * The schedules in the order the list shows them: active first, soonest
 * firing first; then paused, then finished and anything else, each by name.
 *
 * The runtime answers newest first, which puts a schedule created a minute ago
 * above the one about to fire.
 */
export function sortSchedules(schedules: readonly ScheduleView[]): ScheduleView[] {
  const rank = (status: string) => {
    const at = ["active", "paused", "finished"].indexOf(status);
    return at === -1 ? 3 : at;
  };
  const soonest = (schedule: ScheduleView) => {
    const at = timeOf(schedule.next_run_at);
    return Number.isFinite(at) ? at : Number.POSITIVE_INFINITY;
  };
  return [...schedules].sort((a, b) => {
    const byStatus = rank(a.status) - rank(b.status);
    if (byStatus !== 0) return byStatus;
    if (a.status === "active") {
      const [x, y] = [soonest(a), soonest(b)];
      if (x !== y) return x < y ? -1 : 1;
    }
    return a.name.localeCompare(b.name);
  });
}

/**
 * The paragraphs of the delete question.
 *
 * Says what is kept as well as what goes: the tasks a schedule created are
 * records of work done, and an operator deleting a schedule to stop it must not
 * think they are deleting its history too.
 */
export function deleteMessage(schedule: ScheduleView): string {
  return (
    `${schedule.name} stops firing and is removed. This cannot be undone.\n\n` +
    "The tasks it already created are kept, with their traces and audit records. " +
    "Pausing it instead keeps the schedule and stops it firing."
  );
}
