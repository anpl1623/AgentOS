import { describe, expect, it } from "vitest";

import {
  DEFAULT_EXPRESSION,
  PREVIEWED_FIRINGS,
  blankForm,
  cadenceInput,
  cadenceVerdict,
  chosenAgent,
  firstRunAt,
  isDirty,
  previewStops,
  scheduleInput,
} from "./formModel";
import { NOW, cadence, form, minutes, preview } from "./schedulesTestData";
import { fromLocalInput, localMoment, toLocalInput } from "./timeModel";

describe("cadenceInput", () => {
  it("sends only the chosen kind's fields, as the runtime's shape check requires", () => {
    const typed = cadence({ seconds: "7200", expression: "0 9 * * *", clock: "utc" });
    expect(cadenceInput({ ...typed, kind: "every" })).toEqual({
      ok: true,
      value: { kind: "every", seconds: 7200, expression: null, clock: null },
    });
    expect(cadenceInput({ ...typed, kind: "cron" })).toEqual({
      ok: true,
      value: { kind: "cron", seconds: null, expression: "0 9 * * *", clock: "utc" },
    });
    expect(cadenceInput({ ...typed, kind: "once" })).toEqual({
      ok: true,
      value: { kind: "once", seconds: null, expression: null, clock: null },
    });
  });

  it("refuses an interval that is not a whole number of seconds", () => {
    for (const seconds of ["", "  ", "1.5", "-60", "1e3", "sixty", "99999999999999999999"]) {
      expect(cadenceInput(cadence({ kind: "every", seconds })).ok, seconds).toBe(false);
    }
  });

  it("leaves the interval's minimum to the runtime, so the window cannot drift from the CLI", () => {
    expect(cadenceInput(cadence({ kind: "every", seconds: "10" }))).toEqual({
      ok: true,
      value: { kind: "every", seconds: 10, expression: null, clock: null },
    });
  });

  it("trims a cron expression's ends but keeps its own spacing, and refuses an empty one", () => {
    const sent = cadenceInput(cadence({ kind: "cron", expression: "  0 9  * * 1-5 " }));
    expect(sent.ok && sent.value.expression).toBe("0 9  * * 1-5");
    expect(cadenceInput(cadence({ kind: "cron", expression: "   " })).ok).toBe(false);
  });
});

describe("firstRunAt", () => {
  it("lets a recurring cadence compute its own first firing", () => {
    expect(firstRunAt(cadence({ kind: "every" }), NOW)).toEqual({ ok: true, value: null });
    expect(firstRunAt(cadence({ kind: "cron" }), NOW)).toEqual({ ok: true, value: null });
  });

  it("sends a one-shot's local moment as the instant it names", () => {
    const at = new Date(2026, 9, 9, 8, 30).getTime();
    expect(firstRunAt(cadence({ kind: "once", at: "2026-10-09T08:30" }), NOW)).toEqual({
      ok: true,
      value: new Date(at).toISOString(),
    });
  });

  it("refuses a one-shot with no moment, an impossible one, or one already past", () => {
    for (const at of ["", "2026-09-31T09:00", toLocalInput(NOW), toLocalInput(NOW - 60_000)]) {
      expect(firstRunAt(cadence({ kind: "once", at }), NOW).ok, at).toBe(false);
    }
  });
});

describe("blankForm", () => {
  it("starts a one-shot comfortably in the future, on the hour", () => {
    const at = fromLocalInput(blankForm(NOW).cadence.at);
    expect(at).toBe(new Date(2026, 9, 6, 14, 0).getTime());
    const late = new Date(2026, 9, 6, 12, 59).getTime();
    expect(fromLocalInput(blankForm(late).cadence.at) - late).toBeGreaterThan(60 * 60_000);
  });

  it("is valid as soon as it has an agent, a name and an objective", () => {
    expect(scheduleInput(form(), "agent-1", NOW).ok).toBe(true);
    expect(scheduleInput(form({ cadence: cadence({ kind: "once" }) }), "agent-1", NOW).ok).toBe(
      true,
    );
  });

  it("starts a cron expression with day names, which the runtime's Sunday-first count cannot misread", () => {
    expect(blankForm(NOW).cadence.expression).toBe(DEFAULT_EXPRESSION);
    expect(DEFAULT_EXPRESSION).toBe("0 9 * * MON-FRI");
  });
});

describe("scheduleInput", () => {
  it("builds the create input from the form", () => {
    expect(scheduleInput(form({ name: " follow-ups " }), "agent-1", NOW)).toEqual({
      ok: true,
      value: {
        agent_id: "agent-1",
        name: "follow-ups",
        objective: "Draft the overdue follow-ups.",
        cadence: { kind: "every", seconds: 3600, expression: null, clock: null },
        first_run_at: null,
      },
    });
  });

  it("names the first thing missing", () => {
    expect(scheduleInput(form(), "", NOW)).toEqual({
      ok: false,
      error: "Choose an agent to do the work.",
    });
    expect(scheduleInput(form({ name: "  " }), "agent-1", NOW).ok).toBe(false);
    expect(scheduleInput(form({ objective: "" }), "agent-1", NOW).ok).toBe(false);
    const past = cadence({ kind: "once", at: toLocalInput(NOW - 60 * 60_000) });
    expect(scheduleInput(form({ cadence: past }), "agent-1", NOW)).toEqual({
      ok: false,
      error: "That moment has already passed.",
    });
  });
});

describe("chosenAgent", () => {
  const enabled = [{ id: "a" }, { id: "b" }];

  it("keeps a choice that can still be given work", () => {
    expect(chosenAgent("b", enabled)).toBe("b");
  });

  it("falls back to the first enabled agent when the draft's has gone or been disabled", () => {
    expect(chosenAgent("gone", enabled)).toBe("a");
    expect(chosenAgent("", enabled)).toBe("a");
    expect(chosenAgent("a", [])).toBe("");
  });
});

describe("cadenceVerdict", () => {
  const hourly = cadence({ kind: "every", seconds: "3600" });

  it("refuses what the form cannot send without asking the runtime", () => {
    expect(cadenceVerdict(cadence({ kind: "every", seconds: "x" }), NOW, null, null).state).toBe(
      "invalid",
    );
  });

  it("is checking until the runtime has answered for exactly what is typed", () => {
    expect(cadenceVerdict(hourly, NOW, null, null)).toEqual({ state: "checking" });
  });

  it("shows the runtime's refusal verbatim", () => {
    const error =
      "an interval of 10s is shorter than the 60s minimum; each firing is a whole agent run";
    expect(
      cadenceVerdict(hourly, NOW, preview([], { valid: false, error, description: null }), null),
    ).toEqual({ state: "invalid", error });
  });

  it("lists five firings under the runtime's description", () => {
    const runs = [1, 2, 3, 4, 5].map((n) => minutes(n * 60));
    const verdict = cadenceVerdict(hourly, NOW, preview(runs), null);
    expect(verdict).toEqual({
      state: "valid",
      description: "every 3600s",
      heading: "Next five firings:",
      firings: runs.map((iso) => localMoment(Date.parse(iso))),
    });
  });

  it("says when a cron expression has fewer than five firings left, and refuses one with none", () => {
    const yearly = cadence({ kind: "cron", expression: "0 0 1 1 * 2027", clock: "utc" });
    const one = cadenceVerdict(yearly, NOW, preview(["2027-01-01T00:00:00Z"]), null);
    expect(one.state === "valid" && one.heading).toBe("Only one firing is left:");
    expect(one.state === "valid" && one.firings[0]).toContain("(00:00 UTC)");
    const two = cadenceVerdict(yearly, NOW, preview([minutes(60), minutes(120)]), null);
    expect(two.state === "valid" && two.heading).toBe("Only 2 firings are left:");
    expect(cadenceVerdict(yearly, NOW, preview([]), null).state).toBe("invalid");
  });

  it("does not block the form when the check itself failed, but says so", () => {
    expect(cadenceVerdict(hourly, NOW, null, "the runtime is not reachable")).toEqual({
      state: "unchecked",
      error: "the runtime is not reachable",
    });
  });

  it("previews a one-shot from its own moment, without the runtime", () => {
    const at = new Date(2026, 9, 9, 8, 30).getTime();
    const once = cadence({ kind: "once", at: "2026-10-09T08:30" });
    expect(cadenceVerdict(once, NOW, null, null)).toEqual({
      state: "valid",
      description: "once, in 2d",
      heading: "Fires once:",
      firings: [localMoment(at)],
    });
    expect(
      cadenceVerdict(cadence({ kind: "once", at: toLocalInput(NOW - 60_000) }), NOW, null, null)
        .state,
    ).toBe("invalid");
  });
});

describe("previewStops", () => {
  it("keeps as many empty stops as a valid answer would list", () => {
    expect(previewStops("every")).toBe(PREVIEWED_FIRINGS);
    expect(previewStops("cron")).toBe(PREVIEWED_FIRINGS);
    expect(previewStops("once")).toBe(1);
  });
});

describe("isDirty", () => {
  const blank = blankForm(NOW);

  it("counts what a person typed, not the defaults or the moving one-shot moment", () => {
    expect(isDirty(blank, blank)).toBe(false);
    expect(isDirty({ ...blank, agentId: "agent-2" }, blank)).toBe(false);
    const later = { ...blank, cadence: { ...blank.cadence, at: "2030-01-01T00:00" } };
    expect(isDirty(later, blank)).toBe(false);
    expect(isDirty({ ...blank, name: "x" }, blank)).toBe(true);
    expect(isDirty({ ...blank, cadence: { ...blank.cadence, kind: "cron" } }, blank)).toBe(true);
  });
});
