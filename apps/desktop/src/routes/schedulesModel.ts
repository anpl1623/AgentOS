/**
 * What the schedules screen decides, apart from how it draws it.
 *
 * The cadence form and the `CadenceInput` it sends, the moment a one-shot is
 * given, and what each schedule's row claims about when it fires live here as
 * pure functions over an explicit `now`, so a test fails when one of them
 * changes. `Schedules.tsx` only renders what these return.
 *
 * Nothing here judges whether a cadence is acceptable. An interval's minimum
 * and a cron expression's grammar are the runtime's to decide, through
 * `check_cadence`, so the window cannot drift from what the CLI enforces. The
 * form refuses only what cannot be sent at all: a number that is not one, an
 * empty expression, a moment that cannot be read.
 */

import type { CadenceInput } from "../bindings/CadenceInput";
import type { CadencePreview } from "../bindings/CadencePreview";
import type { CadenceView } from "../bindings/CadenceView";
import type { CreateScheduleInput } from "../bindings/CreateScheduleInput";
import type { ScheduleView } from "../bindings/ScheduleView";
import type { VerdictTone } from "../components/status";

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/** A value the form can send, or the reason it cannot. */
export type Checked<T> = { ok: true; value: T } | { ok: false; error: string };

function accept<T>(value: T): Checked<T> {
  return { ok: true, value };
}

function refuse<T>(error: string): Checked<T> {
  return { ok: false, error };
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** Milliseconds since the epoch, or `NaN` for a time that cannot be read. */
function timeOf(iso: string | null | undefined): number {
  return iso ? Date.parse(iso) : Number.NaN;
}

/**
 * A length of time as its largest whole unit: `4m`, `22h`, `6d`.
 *
 * Rounded down, so a firing twenty-two and a half hours away is never said to
 * be twenty-three hours away and then arrive early.
 */
function length(ms: number): string {
  if (ms < HOUR) return `${Math.max(1, Math.floor(ms / MINUTE))}m`;
  if (ms < DAY) return `${Math.floor(ms / HOUR)}h`;
  return `${Math.floor(ms / DAY)}d`;
}

/**
 * How far `iso` lies from `now`, in either direction: `in 22h`, `30m ago`.
 *
 * `ago` in the shared formatter clamps the future to "just now", which is
 * right for things that happened and wrong for every `next_run_at` on this
 * screen. Within a minute either side reads `now`. A time that cannot be read
 * is returned as it came, rather than as a confident nonsense length.
 */
export function relative(iso: string, now: number): string {
  const at = timeOf(iso);
  if (!Number.isFinite(at)) return iso;
  const delta = at - now;
  if (Math.abs(delta) < MINUTE) return "now";
  return delta > 0 ? `in ${length(delta)}` : `${length(-delta)} ago`;
}

/**
 * A moment as a short local date and time: `Tue 7 Oct 09:00`.
 *
 * Built from the date's own fields rather than `toLocaleString`, so it reads
 * the same in every webview and a column of firings lines up.
 */
export function localMoment(ms: number): string {
  const at = new Date(ms);
  const weekday = WEEKDAYS[at.getDay()] ?? "";
  const month = MONTHS[at.getMonth()] ?? "";
  return `${weekday} ${at.getDate()} ${month} ${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

/**
 * One previewed firing, in local time.
 *
 * A cron expression read in UTC also gets its UTC wall time beside it, because
 * that is the clock the operator typed the expression in: `0 9 * * *` should be
 * seen to land on 09:00 UTC, not only on whatever that is here.
 */
export function firingLabel(iso: string, clock: string | null): string {
  const at = timeOf(iso);
  if (!Number.isFinite(at)) return iso;
  const local = localMoment(at);
  if (clock !== "utc") return local;
  const utc = new Date(at);
  return `${local} (${pad(utc.getUTCHours())}:${pad(utc.getUTCMinutes())} UTC)`;
}

/**
 * A moment as an `<input type="datetime-local">` holds it: `2026-10-06T14:30`,
 * in the window's own time zone, to the minute.
 */
export function toLocalInput(ms: number): string {
  const at = new Date(ms);
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `T${pad(at.getHours())}:${pad(at.getMinutes())}`
  );
}

const LOCAL_INPUT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/;

/**
 * The moment a `datetime-local` value names, or `NaN`.
 *
 * Parsed by hand rather than through `Date.parse`, whose reading of a string
 * without an offset has differed between engines. A date that does not exist,
 * such as the thirty-first of September, is refused rather than rolled over
 * into October: a one-shot that fires a day late because of a typo is the
 * silent kind of wrong this screen exists to prevent.
 */
export function fromLocalInput(text: string): number {
  const match = LOCAL_INPUT.exec(text.trim());
  if (!match) return Number.NaN;
  const [year, month, day, hour, minute, second] = match
    .slice(1)
    .map((part) => (part === undefined ? 0 : Number(part)));
  const at = new Date(year!, month! - 1, day!, hour!, minute!, second!);
  const exists =
    at.getFullYear() === year &&
    at.getMonth() === month! - 1 &&
    at.getDate() === day &&
    at.getHours() === hour &&
    at.getMinutes() === minute;
  return exists ? at.getTime() : Number.NaN;
}

/**
 * Seconds as the units a person counts in: `6h`, `1h 30m`, `90s` → `1m 30s`.
 *
 * The runtime describes an interval as `every 21600s`, exactly and
 * unhelpfully; this is said beside it, never instead of it.
 */
export function spanOfSeconds(seconds: number): string {
  if (!Number.isSafeInteger(seconds) || seconds <= 0) return `${seconds}s`;
  const parts: string[] = [];
  let rest = seconds;
  for (const [unit, size] of [
    ["d", 86_400],
    ["h", 3_600],
    ["m", 60],
    ["s", 1],
  ] as const) {
    if (rest >= size) {
      parts.push(`${Math.floor(rest / size)}${unit}`);
      rest %= size;
    }
  }
  return parts.join(" ");
}

// ---------------------------------------------------------------------------
// The cadence form
// ---------------------------------------------------------------------------

/** The cadences a schedule can have, as `CadenceInput.kind` names them. */
export type CadenceKind = "every" | "cron" | "once";

/** The kinds in the order the selector offers them, with their labels. */
export const CADENCE_KINDS: readonly { kind: CadenceKind; label: string }[] = [
  { kind: "every", label: "Interval" },
  { kind: "cron", label: "Cron" },
  { kind: "once", label: "Once" },
];

/** The clock a cron expression is read in. */
export type CronClock = "utc" | "local";

/**
 * The cadence part of the form, as typed.
 *
 * Every kind's fields are kept while another kind is chosen, so switching from
 * Cron to Interval and back does not throw away an expression. Only the chosen
 * kind's fields are sent: the runtime refuses a cadence that names two.
 */
export interface CadenceForm {
  kind: CadenceKind;
  /** Seconds between firings, as typed. */
  seconds: string;
  /** The cron expression, as typed. */
  expression: string;
  clock: CronClock;
  /** The one-shot's moment, as a `datetime-local` value. */
  at: string;
}

/** The whole create form, as typed. */
export interface ScheduleForm {
  agentId: string;
  name: string;
  objective: string;
  cadence: CadenceForm;
}

/**
 * A blank form at `now`.
 *
 * A one-shot starts at the top of the hour after next, so the default is
 * always a moment comfortably in the future rather than one that will have
 * passed by the time the objective is typed. A cron expression starts read in
 * local time: the preview lists firings in local time, and an operator writing
 * `0 9` almost always means nine o'clock where they are.
 */
export function blankForm(now: number): ScheduleForm {
  const hour = new Date(now);
  hour.setMinutes(0, 0, 0);
  return {
    agentId: "",
    name: "",
    objective: "",
    cadence: {
      kind: "every",
      seconds: "3600",
      expression: "0 9 * * 1-5",
      clock: "local",
      at: toLocalInput(hour.getTime() + 2 * HOUR),
    },
  };
}

/**
 * The `CadenceInput` the form names, or why it names none.
 *
 * Fields that belong to the other kinds are sent empty, which is what the
 * runtime's shape check requires. A `once` carries no fields of its own: its
 * moment is the schedule's first run, sent beside the cadence.
 */
export function cadenceInput(form: CadenceForm): Checked<CadenceInput> {
  const empty: CadenceInput = { kind: form.kind, seconds: null, expression: null, clock: null };
  switch (form.kind) {
    case "once":
      return accept(empty);
    case "every": {
      const text = form.seconds.trim();
      if (text === "") return refuse("Say how many seconds apart it fires.");
      if (!/^\d+$/.test(text)) return refuse("An interval is a whole number of seconds.");
      const seconds = Number(text);
      if (!Number.isSafeInteger(seconds)) return refuse("That interval is too long to store.");
      return accept({ ...empty, seconds });
    }
    case "cron": {
      // Inner runs of space are the expression's own; only the ends are noise.
      const expression = form.expression.trim();
      if (expression === "") {
        return refuse(
          "Type a cron expression: five fields (minute hour day month weekday), " +
            "or six with seconds first.",
        );
      }
      return accept({ ...empty, expression, clock: form.clock });
    }
  }
}

/**
 * The first firing to send, as RFC 3339, or `null` to let a recurring cadence
 * compute its own.
 *
 * A one-shot needs a moment, and the moment must be ahead of `now`. The
 * runtime would accept one in the past and fire it on the next tick, which is
 * never what somebody creating a schedule for a particular time meant.
 */
export function firstRunAt(form: CadenceForm, now: number): Checked<string | null> {
  if (form.kind !== "once") return accept(null);
  if (form.at.trim() === "") return refuse("Choose when it fires.");
  const at = fromLocalInput(form.at);
  if (!Number.isFinite(at)) return refuse("That date and time do not exist.");
  if (at <= now) return refuse("That moment has already passed.");
  return accept(new Date(at).toISOString());
}

/**
 * The agent the form will use: the one chosen, if it can still be given work,
 * otherwise the first that can.
 *
 * A draft outlives the screen, and the agent it named may have been disabled
 * in the meantime; the select must not claim a choice the form would not send.
 */
export function chosenAgent(agentId: string, enabled: readonly { id: string }[]): string {
  return enabled.some((agent) => agent.id === agentId) ? agentId : (enabled[0]?.id ?? "");
}

/** The `create_schedule` input the whole form names, or the first reason it names none. */
export function scheduleInput(
  form: ScheduleForm,
  agentId: string,
  now: number,
): Checked<CreateScheduleInput> {
  if (agentId === "") return refuse("Choose an agent to do the work.");
  const name = form.name.trim();
  if (name === "") return refuse("Give the schedule a name.");
  const objective = form.objective.trim();
  if (objective === "") return refuse("Say what each firing should do.");
  const cadence = cadenceInput(form.cadence);
  if (!cadence.ok) return cadence;
  const first = firstRunAt(form.cadence, now);
  if (!first.ok) return first;
  return accept({
    agent_id: agentId,
    name,
    objective,
    cadence: cadence.value,
    first_run_at: first.value,
  });
}

/**
 * How many firings the runtime's preview lists. Fewer means the cadence has
 * no more than that left.
 */
export const PREVIEWED_FIRINGS = 5;

/**
 * What the preview beneath the cadence says.
 *
 * - `invalid`: the form, or the runtime, refused it, and says why verbatim.
 * - `checking`: the runtime has not answered for what is typed now. An answer
 *   for an earlier keystroke is never shown as the answer for this one.
 * - `unchecked`: the check itself failed; the runtime will still check on
 *   create, so the form is not blocked, but the operator is told.
 * - `valid`: the runtime's description, the next firings, and a heading that
 *   says how many there are. A cron expression with fewer than five left says
 *   "only"; one with none left is refused, since it would never fire.
 */
export type CadenceVerdict =
  | { state: "invalid"; error: string }
  | { state: "checking" }
  | { state: "unchecked"; error: string }
  | { state: "valid"; description: string; heading: string; firings: string[] };

/**
 * Read the preview for the form as it stands.
 *
 * `preview` must be the runtime's answer for exactly the cadence in `form`, or
 * `null` while that answer is outstanding. A one-shot is not sent to the
 * runtime at all: its whole preview is the moment chosen, checked here.
 */
export function cadenceVerdict(
  form: CadenceForm,
  now: number,
  preview: CadencePreview | null,
  checkError: string | null,
): CadenceVerdict {
  const local = cadenceInput(form);
  if (!local.ok) return { state: "invalid", error: local.error };

  if (form.kind === "once") {
    const first = firstRunAt(form, now);
    if (!first.ok) return { state: "invalid", error: first.error };
    const at = timeOf(first.value);
    return {
      state: "valid",
      description: `once, ${relative(first.value ?? "", now)}`,
      heading: "Fires once:",
      firings: [localMoment(at)],
    };
  }

  if (checkError !== null) return { state: "unchecked", error: checkError };
  if (preview === null) return { state: "checking" };
  if (!preview.valid) {
    return { state: "invalid", error: preview.error ?? "The runtime refused this cadence." };
  }
  const count = preview.next_runs.length;
  if (count === 0) {
    return { state: "invalid", error: "It has no occurrence after now, so it would never fire." };
  }
  const clock = form.kind === "cron" ? form.clock : null;
  return {
    state: "valid",
    description: preview.description ?? "",
    heading:
      count >= PREVIEWED_FIRINGS
        ? "Next five firings:"
        : count === 1
          ? "Only one firing is left:"
          : `Only ${count} firings are left:`,
    firings: preview.next_runs.map((iso) => firingLabel(iso, clock)),
  };
}

/**
 * The form differs from a blank one in anything a person typed.
 *
 * The cadence's defaults are not counted: changing the interval from one hour
 * to two and back is not work worth keeping a draft for. The one-shot's moment
 * is ignored too, because a blank form's default moves with the clock.
 */
export function isDirty(form: ScheduleForm, blank: ScheduleForm): boolean {
  return (
    form.name.trim() !== "" ||
    form.objective.trim() !== "" ||
    form.cadence.kind !== blank.cadence.kind ||
    form.cadence.seconds !== blank.cadence.seconds ||
    form.cadence.expression !== blank.cadence.expression ||
    form.cadence.clock !== blank.cadence.clock
  );
}

// ---------------------------------------------------------------------------
// The list
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
        next: ahead ? `next ${relative(nextAt, now)}` : `overdue by ${length(now - nextTime)}`,
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
