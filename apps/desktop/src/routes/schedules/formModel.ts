/**
 * The create form: what is typed, what it sends, and what it says about it.
 *
 * The cadence form and the `CadenceInput` it sends, the moment a one-shot is
 * given, and what the preview beneath the cadence claims live here as pure
 * functions over an explicit `now`, so a test fails when one of them changes.
 * `CreateSchedule.tsx` only renders what these return.
 *
 * What each field says is wrong, and whether Create can be pressed, are in
 * `validationModel.ts`.
 *
 * Nothing here judges whether a cadence is acceptable. An interval's minimum
 * and a cron expression's grammar are the runtime's to decide, through
 * `check_cadence`, so the window cannot drift from what the CLI enforces. The
 * form refuses only what cannot be sent at all: a number that is not one, an
 * empty expression, a moment that cannot be read, a name already taken.
 */

import type { CadenceInput } from "../../bindings/CadenceInput";
import type { CadencePreview } from "../../bindings/CadencePreview";
import type { CreateScheduleInput } from "../../bindings/CreateScheduleInput";
import {
  HOUR,
  firingLabel,
  fromLocalInput,
  localMoment,
  relative,
  timeOf,
  toLocalInput,
} from "./timeModel";

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
// The cadence form
// ---------------------------------------------------------------------------

/** The cadences a schedule can have, as `CadenceInput.kind` names them. */
export type CadenceKind = "every" | "cron" | "once";

/** The kinds in the order the selector offers them, with their labels and a line on each. */
export const CADENCE_KINDS: readonly { kind: CadenceKind; label: string; note: string }[] = [
  { kind: "every", label: "Interval", note: "A fixed gap between firings" },
  { kind: "cron", label: "Cron", note: "On a calendar pattern" },
  { kind: "once", label: "Once", note: "At one moment, then done" },
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
 * The cron expression a blank form starts with: nine o'clock on weekdays.
 *
 * Written with day names, not `1-5`. The runtime's cron grammar counts the
 * days of the week from Sunday as 1, so `1-5` there is Sunday to Thursday, the
 * opposite of what anyone used to `crontab` would read it as. Names mean the
 * same thing in both numberings.
 */
export const DEFAULT_EXPRESSION = "0 9 * * MON-FRI";

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
      expression: DEFAULT_EXPRESSION,
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
 * How many empty stops the preview's timeline keeps while it has no firings to
 * show: as many as a valid answer would list. Typing a cron expression moves
 * the preview between checking and refused on every pause, and a timeline that
 * kept its height through both does not make the form below it jump.
 */
export function previewStops(kind: CadenceKind): number {
  return kind === "once" ? 1 : PREVIEWED_FIRINGS;
}

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
