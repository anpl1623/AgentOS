/**
 * The create form's inline validation: what each field says is wrong, when it
 * says it, and whether Create can be pressed.
 *
 * Pure functions over the form and the cadence's verdict (`formModel.ts`), so
 * a test fails when what the form says, or when it says it, changes.
 */

import type { CreateScheduleInput } from "../../bindings/CreateScheduleInput";
import type { CadenceVerdict, Checked, ScheduleForm } from "./formModel";

/**
 * The error drawn directly beneath the cadence field, or `null`.
 *
 * Only a refusal is an error. A check still in flight is not one, and neither
 * is a check that could not be made: the runtime checks again on create, so
 * the preview says that in its own quieter words instead.
 */
export function cadenceFieldError(verdict: CadenceVerdict): string | null {
  return verdict.state === "invalid" ? verdict.error : null;
}

/**
 * Why the typed name cannot be used because another schedule has it, or `null`.
 *
 * Names are unique in the runtime's store, compared exactly as stored, so the
 * comparison here is exact too: `Follow-ups` and `follow-ups` are two names
 * there, and saying they clash would refuse something the runtime accepts.
 */
export function nameClash(name: string, taken: readonly string[]): string | null {
  const trimmed = name.trim();
  if (trimmed === "" || !taken.includes(trimmed)) return null;
  return `A schedule called ${trimmed} already exists. Names are unique.`;
}

/** The text fields, by whether the person has been into one and left it. */
export interface FieldsLeft {
  name: boolean;
  objective: boolean;
}

/** The message to show under each text field, or `null` for none yet. */
export interface FieldErrors {
  name: string | null;
  objective: string | null;
}

/**
 * The message under each text field.
 *
 * A field that is merely empty says so once it has been left, not while it is
 * first being typed into: an error on a field nobody has reached yet is a nag.
 * A name another schedule already has is said at once, because that is news
 * the person needs before they finish typing. Only the fields typed into
 * freely are here; the cadence says what is wrong as it is typed (see
 * `cadenceFieldError`), because its refusals come from the runtime while the
 * field still has focus.
 */
export function fieldErrors(
  form: ScheduleForm,
  taken: readonly string[],
  left: FieldsLeft,
): FieldErrors {
  const clash = nameClash(form.name, taken);
  const name = form.name.trim() === "" ? "Give the schedule a name." : clash;
  const objective = form.objective.trim() === "" ? "Say what each firing should do." : null;
  return {
    name: left.name || clash !== null ? name : null,
    objective: left.objective ? objective : null,
  };
}

/**
 * Whether Create can be pressed, and the one reason beside it when it cannot.
 *
 * The reason names the first thing missing, in the order the form is read. A
 * cadence the runtime or the form refused is already explained beneath the
 * cadence, so the reason points there instead of repeating it.
 */
export function submitState(
  ready: Checked<CreateScheduleInput>,
  verdict: CadenceVerdict,
  clash: string | null,
): { disabled: boolean; reason: string | null } {
  const cadenceRefused = verdict.state === "invalid";
  let reason: string | null = null;
  if (!ready.ok) {
    reason = cadenceRefused && ready.error === verdict.error ? FIX_CADENCE : ready.error;
  } else if (clash !== null) {
    reason = clash;
  } else if (cadenceRefused) {
    reason = FIX_CADENCE;
  }
  return { disabled: reason !== null, reason };
}

const FIX_CADENCE = "The cadence needs fixing first.";
