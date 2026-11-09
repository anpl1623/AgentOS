import { describe, expect, it } from "vitest";

import {
  cadenceLabel,
  cycleProgress,
  deleteMessage,
  scheduleCard,
  scheduleCardId,
  scheduleRow,
  scheduleTone,
  sortSchedules,
} from "./schedulesModel";
import { NOW, minutes, schedule } from "./schedulesTestData";
import { localMoment } from "./timeModel";

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

describe("scheduleCardId", () => {
  it("gives each schedule's card an id of its own", () => {
    expect(scheduleCardId("s-1")).toBe("schedule-card-s-1");
    expect(scheduleCardId("s-1")).not.toBe(scheduleCardId("s-2"));
  });
});

describe("deleteMessage", () => {
  it("says the tasks the schedule created are kept", () => {
    expect(deleteMessage(schedule())).toMatch(/tasks it already created are kept/);
  });
});

describe("cycleProgress", () => {
  it("measures the wait from the last firing to the next", () => {
    const halfway = schedule({ last_run_at: minutes(-60), next_run_at: minutes(60) });
    expect(cycleProgress(halfway, NOW)).toBeCloseTo(0.5);
  });

  it("measures from creation when it has never fired", () => {
    // Created 600 minutes ago, due in 90: 600 of 690 minutes have passed.
    expect(cycleProgress(schedule(), NOW)).toBeCloseTo(600 / 690);
  });

  it("stays between empty and full, and draws nothing from times it cannot read", () => {
    expect(cycleProgress(schedule({ next_run_at: minutes(-5) }), NOW)).toBe(1);
    expect(cycleProgress(schedule({ next_run_at: null }), NOW)).toBeNull();
    expect(cycleProgress(schedule({ last_run_at: minutes(30) }), NOW)).toBe(0);
    expect(cycleProgress(schedule({ last_run_at: minutes(120) }), NOW)).toBeNull();
  });
});

describe("scheduleCard", () => {
  it("counts down to an active schedule's next firing, with the moment beneath", () => {
    const card = scheduleCard(schedule({ next_run_at: minutes(22 * 60 + 28) }), NOW, true);
    expect(card.when).toMatchObject({
      label: "Next firing",
      figure: "22h 28m",
      at: localMoment(NOW + (22 * 60 + 28) * 60_000),
      caption: null,
      tone: "normal",
    });
    expect(card.when.progress).not.toBeNull();
  });

  it("says a countdown is only a due time while the scheduler is off", () => {
    expect(scheduleCard(schedule(), NOW, false).when.caption).toBe(
      "Due then, but the scheduler is off.",
    );
    expect(scheduleCard(schedule(), NOW, null).when.caption).toBeNull();
  });

  it("shows how overdue a schedule is, in the attention tone, and what happens next", () => {
    const late = schedule({ next_run_at: minutes(-30) });
    expect(scheduleCard(late, NOW, false).when).toMatchObject({
      label: "Overdue by",
      figure: "30m",
      tone: "warn",
      progress: null,
      caption: "It fires once when the scheduler starts.",
    });
    expect(scheduleCard(late, NOW, true).when.caption).toBe(
      "It fires on the scheduler's next check.",
    );
  });

  it("counts nothing down for a paused or finished schedule, and says why in the row's words", () => {
    const paused = scheduleCard(schedule({ status: "paused", next_run_at: minutes(90) }), NOW, true);
    expect(paused.when).toMatchObject({ figure: null, caption: "Would fire in 1h", tone: "quiet" });
    expect(paused.when.at).not.toBeNull();
    const finished = scheduleCard(schedule({ status: "finished", next_run_at: null }), NOW, true);
    expect(finished.when).toMatchObject({
      figure: null,
      at: null,
      caption: "Will not fire again",
    });
  });

  it("says the cadence in words and keeps the runtime's description beneath", () => {
    const card = scheduleCard(
      schedule({
        cadence: {
          kind: "every",
          seconds: 21_600,
          expression: null,
          clock: null,
          description: "every 21600s",
        },
      }),
      NOW,
      true,
    );
    expect(card.words).toBe("Every 6 hours");
    expect(card.exact).toBe("every 21600s");
  });

  it("falls back to the runtime's description for a cron expression it has no words for", () => {
    const card = scheduleCard(
      schedule({
        cadence: {
          kind: "cron",
          seconds: null,
          expression: "0 9-17 * * *",
          clock: "utc",
          description: "cron `0 9-17 * * *` (utc)",
        },
      }),
      NOW,
      true,
    );
    expect(card).toMatchObject({ words: "cron `0 9-17 * * *` (utc)", exact: null });
  });

  it("says when it last fired, or that it never has", () => {
    expect(scheduleCard(schedule(), NOW, true)).toMatchObject({
      lastFired: "never",
      lastFiredAt: null,
    });
    const fired = scheduleCard(schedule({ last_run_at: minutes(-92) }), NOW, true);
    expect(fired.lastFired).toBe("1h ago");
    expect(fired.lastFiredAt).toBe(localMoment(NOW - 92 * 60_000));
  });
});
