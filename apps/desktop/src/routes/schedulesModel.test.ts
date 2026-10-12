import { describe, expect, it } from "vitest";

import type { CadencePreview } from "../bindings/CadencePreview";
import type { ScheduleView } from "../bindings/ScheduleView";
import {
  type CadenceForm,
  type ScheduleForm,
  blankForm,
  cadenceInput,
  cadenceLabel,
  cadenceVerdict,
  chosenAgent,
  deleteMessage,
  firingLabel,
  firstRunAt,
  fromLocalInput,
  isDirty,
  localMoment,
  relative,
  scheduleInput,
  scheduleRow,
  scheduleTone,
  sortSchedules,
  spanOfSeconds,
  toLocalInput,
} from "./schedulesModel";

// Built from local fields, so every assertion below holds in any time zone the
// tests happen to run in.
const NOW = new Date(2026, 9, 6, 12, 0, 0).getTime();
const minutes = (n: number) => new Date(NOW + n * 60_000).toISOString();

function cadence(fields: Partial<CadenceForm> = {}): CadenceForm {
  return { ...blankForm(NOW).cadence, ...fields };
}

function form(fields: Partial<ScheduleForm> = {}): ScheduleForm {
  return {
    ...blankForm(NOW),
    agentId: "agent-1",
    name: "follow-ups",
    objective: "Draft the overdue follow-ups.",
    ...fields,
  };
}

function schedule(fields: Partial<ScheduleView> = {}): ScheduleView {
  return {
    id: "s-1",
    name: "follow-ups",
    agent_id: "agent-1",
    agent_name: "sales",
    objective: "Draft the overdue follow-ups.",
    cadence: {
      kind: "every",
      seconds: 3600,
      expression: null,
      clock: null,
      description: "every 3600s",
    },
    status: "active",
    next_run_at: minutes(90),
    last_run_at: null,
    last_task_id: null,
    created_at: minutes(-600),
    ...fields,
  };
}

function preview(next_runs: string[], fields: Partial<CadencePreview> = {}): CadencePreview {
  return { valid: true, error: null, description: "every 3600s", next_runs, ...fields };
}

describe("relative", () => {
  it("speaks of the future as well as the past, which `ago` cannot", () => {
    expect(relative(minutes(90), NOW)).toBe("in 1h");
    expect(relative(minutes(-30), NOW)).toBe("30m ago");
    expect(relative(minutes(60 * 24 * 6 + 600), NOW)).toBe("in 6d");
  });

  it("rounds down, so a firing never arrives before the time it was said to be", () => {
    expect(relative(minutes(22 * 60 + 59), NOW)).toBe("in 22h");
    expect(relative(minutes(59), NOW)).toBe("in 59m");
  });

  it("reads a moment within a minute either side as now", () => {
    expect(relative(new Date(NOW + 30_000).toISOString(), NOW)).toBe("now");
    expect(relative(new Date(NOW - 30_000).toISOString(), NOW)).toBe("now");
  });

  it("returns a time it cannot read as it came", () => {
    expect(relative("not a time", NOW)).toBe("not a time");
  });
});

describe("local moments", () => {
  it("round-trips a datetime-local value to the minute", () => {
    const at = new Date(2026, 9, 7, 9, 5).getTime();
    expect(toLocalInput(at)).toBe("2026-10-07T09:05");
    expect(fromLocalInput("2026-10-07T09:05")).toBe(at);
    expect(fromLocalInput(toLocalInput(NOW))).toBe(NOW);
  });

  it("refuses a date that does not exist rather than rolling it into the next month", () => {
    expect(fromLocalInput("2026-09-31T09:00")).toBeNaN();
    expect(fromLocalInput("2026-02-29T09:00")).toBeNaN();
    expect(fromLocalInput("2026-10-07T24:00")).toBeNaN();
    expect(fromLocalInput("next tuesday")).toBeNaN();
    expect(fromLocalInput("")).toBeNaN();
  });

  it("formats a firing the same way in every webview", () => {
    expect(localMoment(new Date(2026, 9, 7, 9, 0).getTime())).toBe("Wed 7 Oct 09:00");
  });

  it("gives a UTC cron's firings their UTC wall time too, and a local one's not", () => {
    const iso = "2026-10-07T09:00:00Z";
    const local = localMoment(Date.parse(iso));
    expect(firingLabel(iso, "utc")).toBe(`${local} (09:00 UTC)`);
    expect(firingLabel(iso, "local")).toBe(local);
    expect(firingLabel(iso, null)).toBe(local);
  });
});

describe("spanOfSeconds", () => {
  it("says an interval in the units a person counts in", () => {
    expect(spanOfSeconds(21_600)).toBe("6h");
    expect(spanOfSeconds(5_400)).toBe("1h 30m");
    expect(spanOfSeconds(90)).toBe("1m 30s");
    expect(spanOfSeconds(86_400 * 2 + 1)).toBe("2d 1s");
    expect(spanOfSeconds(45)).toBe("45s");
  });

  it("leaves a value that is not a positive whole number as seconds", () => {
    expect(spanOfSeconds(0)).toBe("0s");
    expect(spanOfSeconds(1.5)).toBe("1.5s");
  });
});

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

describe("scheduleRow", () => {
  it("says when an active schedule next fires and that it has never fired", () => {
    const row = scheduleRow(schedule(), NOW);
    expect(row).toMatchObject({
      tone: "ok",
      label: "Active",
      next: "next in 1h",
      overdue: false,
      last: "never fired",
      action: "pause",
      resumeNote: null,
    });
  });

  it("marks an active schedule whose moment has passed as overdue", () => {
    const row = scheduleRow(schedule({ next_run_at: minutes(-30), last_run_at: minutes(-90) }), NOW);
    expect(row.overdue).toBe(true);
    expect(row.next).toBe("overdue by 30m");
    expect(row.last).toBe("last fired 1h ago");
  });

  it("tells a paused schedule's operator that resuming does not replay a backlog", () => {
    const row = scheduleRow(schedule({ status: "paused", next_run_at: minutes(-30) }), NOW);
    expect(row.action).toBe("resume");
    expect(row.next).toBe("not firing");
    expect(row.resumeNote).toMatch(/next occurrence from now/);
    expect(row.resumeNote).toMatch(/not made up/);
  });

  it("keeps a paused schedule's slot when it is still ahead, and says so", () => {
    const row = scheduleRow(schedule({ status: "paused", next_run_at: minutes(90) }), NOW);
    expect(row.next).toBe("would fire in 1h");
    expect(row.resumeNote).toMatch(/keeps that firing/);
  });

  it("says a paused one-shot whose moment passed is finished by resuming, not fired", () => {
    const once = schedule({
      status: "paused",
      next_run_at: minutes(-30),
      cadence: { kind: "once", seconds: null, expression: null, clock: null, description: "once" },
    });
    expect(scheduleRow(once, NOW).resumeNote).toMatch(/finishes it rather than firing it/);
  });

  it("offers neither pause nor resume on a finished schedule, or on a status it does not know", () => {
    const finished = scheduleRow(schedule({ status: "finished", next_run_at: null }), NOW);
    expect(finished).toMatchObject({ action: null, next: "will not fire again", tone: "neutral" });
    const unknown = scheduleRow(schedule({ status: "suspended" }), NOW);
    expect(unknown).toMatchObject({ action: null, tone: "neutral", label: "Suspended" });
  });

  it("answers a prototype key with the neutral tone, not a function from the prototype", () => {
    expect(scheduleTone("constructor")).toBe("neutral");
    expect(scheduleTone("toString")).toBe("neutral");
  });
});

describe("cadenceLabel", () => {
  it("keeps the runtime's description and says an interval in hours beside it", () => {
    const every = schedule().cadence;
    expect(cadenceLabel({ ...every, seconds: 21_600, description: "every 21600s" })).toBe(
      "every 21600s (6h)",
    );
    expect(cadenceLabel({ ...every, seconds: 45, description: "every 45s" })).toBe("every 45s");
    const cron = {
      kind: "cron",
      seconds: null,
      expression: "0 9 * * 1-5",
      clock: "local",
      description: "cron `0 9 * * 1-5` (local)",
    };
    expect(cadenceLabel(cron)).toBe("cron `0 9 * * 1-5` (local)");
  });
});

describe("sortSchedules", () => {
  it("puts active schedules first, soonest firing first, then paused and finished by name", () => {
    const sorted = sortSchedules([
      schedule({ id: "1", name: "b-finished", status: "finished", next_run_at: null }),
      schedule({ id: "2", name: "z-paused", status: "paused" }),
      schedule({ id: "3", name: "later", next_run_at: minutes(600) }),
      schedule({ id: "4", name: "a-paused", status: "paused" }),
      schedule({ id: "5", name: "sooner", next_run_at: minutes(5) }),
      schedule({ id: "6", name: "unreadable", next_run_at: "garbage" }),
    ]);
    expect(sorted.map((each) => each.name)).toEqual([
      "sooner",
      "later",
      "unreadable",
      "a-paused",
      "z-paused",
      "b-finished",
    ]);
  });
});

describe("deleteMessage", () => {
  it("says the tasks the schedule created are kept", () => {
    expect(deleteMessage(schedule())).toMatch(/tasks it already created are kept/);
  });
});
