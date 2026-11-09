import { describe, expect, it } from "vitest";

import { cadenceVerdict, scheduleInput } from "./formModel";
import { NOW, cadence, form, minutes, preview } from "./schedulesTestData";
import { cadenceFieldError, fieldErrors, nameClash, submitState } from "./validationModel";

describe("cadenceFieldError", () => {
  it("puts a refusal beneath the field, verbatim", () => {
    const refused = cadenceVerdict(cadence({ kind: "every", seconds: "1.5" }), NOW, null, null);
    expect(cadenceFieldError(refused)).toBe("An interval is a whole number of seconds.");
  });

  it("is quiet while the check is out, when it could not be made, and when it passed", () => {
    const hourly = cadence({ kind: "every", seconds: "3600" });
    expect(cadenceFieldError(cadenceVerdict(hourly, NOW, null, null))).toBeNull();
    expect(cadenceFieldError(cadenceVerdict(hourly, NOW, null, "unreachable"))).toBeNull();
    const once = cadence({ kind: "once", at: "2026-10-09T08:30" });
    expect(cadenceFieldError(cadenceVerdict(once, NOW, null, null))).toBeNull();
  });
});

describe("nameClash", () => {
  const taken = ["follow-ups", "log-sweep"];

  it("names a clash with a schedule that exists, comparing the trimmed name exactly", () => {
    expect(nameClash(" follow-ups ", taken)).toBe(
      "A schedule called follow-ups already exists. Names are unique.",
    );
  });

  it("does not call a name in another case a clash, because the store would not", () => {
    expect(nameClash("Follow-ups", taken)).toBeNull();
    expect(nameClash("new-one", taken)).toBeNull();
    expect(nameClash("   ", taken)).toBeNull();
  });
});

describe("fieldErrors", () => {
  const left = { name: true, objective: true };
  const untouched = { name: false, objective: false };

  it("says what each text field is missing once it has been left", () => {
    expect(fieldErrors(form({ name: " ", objective: "" }), [], left)).toEqual({
      name: "Give the schedule a name.",
      objective: "Say what each firing should do.",
    });
    expect(fieldErrors(form(), [], left)).toEqual({ name: null, objective: null });
  });

  it("does not nag about a field nobody has reached yet", () => {
    expect(fieldErrors(form({ name: "", objective: "" }), [], untouched)).toEqual({
      name: null,
      objective: null,
    });
  });

  it("says at once that a name is taken, before the field is left", () => {
    expect(fieldErrors(form(), ["follow-ups"], untouched).name).toMatch(/already exists/);
  });
});

describe("submitState", () => {
  const valid = cadenceVerdict(
    cadence({ kind: "every", seconds: "3600" }),
    NOW,
    preview([minutes(60)]),
    null,
  );

  it("lets a complete form be created", () => {
    expect(submitState(scheduleInput(form(), "agent-1", NOW), valid, null)).toEqual({
      disabled: false,
      reason: null,
    });
  });

  it("names the first thing missing, in the order the form is read", () => {
    const ready = scheduleInput(form({ name: "" }), "agent-1", NOW);
    expect(submitState(ready, valid, null)).toEqual({
      disabled: true,
      reason: "Give the schedule a name.",
    });
  });

  it("holds back a name that is taken", () => {
    const clash = nameClash("follow-ups", ["follow-ups"]);
    expect(submitState(scheduleInput(form(), "agent-1", NOW), valid, clash)).toEqual({
      disabled: true,
      reason: clash,
    });
  });

  it("points at the cadence rather than repeating the refusal shown beneath it", () => {
    const typed = cadence({ kind: "every", seconds: "x" });
    const local = cadenceVerdict(typed, NOW, null, null);
    const ready = scheduleInput(form({ cadence: typed }), "agent-1", NOW);
    expect(submitState(ready, local, null)).toEqual({
      disabled: true,
      reason: "The cadence needs fixing first.",
    });
    // The runtime refused a cadence the form itself could send.
    const refused = cadenceVerdict(
      cadence({ kind: "every", seconds: "10" }),
      NOW,
      preview([], { valid: false, error: "too short", description: null }),
      null,
    );
    expect(submitState(scheduleInput(form(), "agent-1", NOW), refused, null).reason).toBe(
      "The cadence needs fixing first.",
    );
  });

  it("does not block on a check that failed, since the runtime checks again on create", () => {
    const unchecked = cadenceVerdict(cadence(), NOW, null, "not reachable");
    expect(submitState(scheduleInput(form(), "agent-1", NOW), unchecked, null).disabled).toBe(
      false,
    );
  });
});
