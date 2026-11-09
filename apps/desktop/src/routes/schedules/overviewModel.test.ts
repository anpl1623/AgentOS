import { describe, expect, it } from "vitest";

import {
  countHints,
  overdueNote,
  schedulerFacts,
  soonestTile,
  summarise,
} from "./overviewModel";
import { NOW, minutes, schedule, scheduler } from "./schedulesTestData";

describe("summarise", () => {
  it("counts the list the way its cards read it", () => {
    const summary = summarise(
      [
        schedule({ id: "1", name: "soon", next_run_at: minutes(5) }),
        schedule({ id: "2", name: "late", next_run_at: minutes(-30) }),
        schedule({ id: "3", name: "later", next_run_at: minutes(600) }),
        schedule({ id: "4", name: "resting", status: "paused", next_run_at: minutes(1) }),
        schedule({ id: "5", name: "done", status: "finished", next_run_at: null }),
      ],
      NOW,
    );
    expect(summary).toEqual({
      active: 3,
      paused: 1,
      overdue: 1,
      soonest: { name: "soon", figure: "5m" },
    });
  });

  it("has no soonest firing when nothing active is waiting", () => {
    expect(summarise([schedule({ status: "paused" })], NOW).soonest).toBeNull();
    expect(summarise([], NOW)).toEqual({ active: 0, paused: 0, overdue: 0, soonest: null });
  });
});

describe("countHints", () => {
  const some = { active: 2, paused: 1, overdue: 1, soonest: null };
  const none = { active: 0, paused: 0, overdue: 0, soonest: null };

  it("says active schedules are held, not firing, while the scheduler is off", () => {
    expect(countHints(some, false).active).toBe("held while the scheduler is off");
    expect(countHints(some, true).active).toBe("firing on their cadence");
    expect(countHints(some, null).active).toBe("set to fire");
  });

  it("says what overdue schedules will do, once each", () => {
    expect(countHints(some, true).overdue).toBe("each fires on the next check");
    expect(countHints(some, false).overdue).toBe("each fires once when it starts");
  });

  it("says plainly when a count is zero", () => {
    expect(countHints(none, true)).toEqual({
      active: "none set to fire",
      paused: "none held back",
      overdue: "nothing past due",
    });
    expect(countHints(some, true).paused).toBe("kept, not firing");
  });
});

describe("soonestTile", () => {
  const summary = summarise([schedule({ name: "soon", next_run_at: minutes(5) })], NOW);

  it("names the soonest schedule, and says when nothing will act on it", () => {
    expect(soonestTile(summary, true)).toEqual({ value: "5m", hint: "soon" });
    expect(soonestTile(summary, null)).toEqual({ value: "5m", hint: "soon" });
    expect(soonestTile(summary, false)).toEqual({ value: "5m", hint: "soon · scheduler off" });
  });

  it("says plainly when nothing is waiting", () => {
    expect(soonestTile(summarise([], NOW), true)).toEqual({
      value: "none",
      hint: "nothing active is waiting",
    });
  });
});

describe("the scheduler", () => {
  it("says what overdue schedules do when it starts, once each", () => {
    expect(overdueNote(0)).toBeNull();
    expect(overdueNote(1)).toMatch(/^One schedule is overdue; it fires once/);
    expect(overdueNote(3)).toMatch(/^3 schedules are overdue; each fires once/);
  });

  it("lists how often it checks, how many runs it allows, and since when", () => {
    expect(schedulerFacts(scheduler(), NOW)).toEqual([
      "Checks every 30s",
      "One run at a time",
      "Started 2h ago",
    ]);
    expect(
      schedulerFacts(scheduler({ max_concurrent_runs: 3, started_at: null, tick_seconds: 90 }), NOW),
    ).toEqual(["Checks every 1m 30s", "Up to 3 runs at once"]);
  });
});
