import { describe, expect, it } from "vitest";

import {
  parseCount,
  readPacing,
  schedulerFacts,
  schedulerNotices,
  schedulerSummary,
} from "./schedulerModel";
import { asSentence } from "./settingsModel";
import { NOW, minutesFrom, scheduler } from "./testData";

describe("parseCount and readPacing", () => {
  it("accepts whole numbers only, leaving the range to the runtime", () => {
    expect(parseCount("30")).toBe(30);
    expect(parseCount(" 5 ")).toBe(5);
    // Zero is a whole number; the runtime refuses it in its own words.
    expect(parseCount("0")).toBe(0);
    for (const text of ["", "1.5", "-3", "1e3", "abc", "99999999999999999999"]) {
      expect(parseCount(text)).toBeNull();
    }
  });

  it("reports pacing as unchanged when the fields say what the scheduler does", () => {
    expect(readPacing(scheduler(), "30", "1")).toEqual({
      tick: 30,
      max: 1,
      valid: true,
      changed: false,
    });
  });

  it("reports a change, and an invalid field as one that cannot be sent", () => {
    expect(readPacing(scheduler(), "60", "1").changed).toBe(true);
    const invalid = readPacing(scheduler(), "", "1");
    expect(invalid.valid).toBe(false);
    expect(invalid.changed).toBe(true);
  });
});

describe("schedulerSummary", () => {
  it("says, when off, that schedules are kept and that nothing fires", () => {
    const summary = schedulerSummary(scheduler(), NOW);
    expect(summary).toMatch(/^Off\./);
    expect(summary).toContain("Schedules are kept");
    expect(summary).toContain("nothing fires");
  });

  it("says, when running, since when and how it is paced", () => {
    const summary = schedulerSummary(
      scheduler({ running: true, started_at: minutesFrom(-90), max_concurrent_runs: 3 }),
      NOW,
    );
    expect(summary).toBe(
      "Running, started 1h ago. It looks for due work every 30s, with up to 3 runs at once.",
    );
  });
});

describe("schedulerFacts", () => {
  const value = (facts: ReturnType<typeof schedulerFacts>, label: string) =>
    facts.find((fact) => fact.label === label);

  it("flags overdue schedules and unreachable tasks only when there are some", () => {
    const quiet = schedulerFacts(scheduler(), NOW);
    expect(quiet.every((fact) => !fact.attention)).toBe(true);
    const loud = schedulerFacts(scheduler({ overdue: 2, unreachable_tasks: 1 }), NOW);
    expect(value(loud, "Overdue")).toEqual({ label: "Overdue", value: "2", attention: true });
    expect(value(loud, "Can never start")?.attention).toBe(true);
  });

  it("says when the next schedule fires, and that a passed one is due", () => {
    expect(value(schedulerFacts(scheduler(), NOW), "Next firing")?.value).toBe("in 30m");
    const passed = scheduler({ next_fire_at: minutesFrom(-5) });
    expect(value(schedulerFacts(passed, NOW), "Next firing")?.value).toBe("due 5m ago");
    expect(
      value(schedulerFacts({ ...passed, running: true }, NOW), "Next firing")?.value,
    ).toBe("due now");
    expect(
      value(schedulerFacts(scheduler({ next_fire_at: null }), NOW), "Next firing")?.value,
    ).toBe("nothing scheduled");
  });
});

describe("schedulerNotices", () => {
  const LEASE =
    "the scheduler is already running in another process on this installation, most likely " +
    "`agentos schedule run` in a terminal.";

  it("shows the lease refusal once, though the switch and the status both carry it", () => {
    expect(schedulerNotices(scheduler({ error: LEASE }), LEASE)).toEqual([asSentence(LEASE)]);
  });

  it("shows a stop the scheduler came to on its own, with nobody pressing anything", () => {
    expect(schedulerNotices(scheduler({ error: "the database went away" }), null)).toEqual([
      "The database went away.",
    ]);
  });

  it("keeps a refusal the status does not repeat beside the status's own error", () => {
    expect(
      schedulerNotices(scheduler({ error: LEASE }), "a tick shorter than 5 seconds is a busy loop"),
    ).toEqual(["A tick shorter than 5 seconds is a busy loop.", asSentence(LEASE)]);
  });

  it("drops an old error once the scheduler is running again", () => {
    expect(schedulerNotices(scheduler({ running: true, error: LEASE }), null)).toEqual([]);
    expect(schedulerNotices(null, null)).toEqual([]);
  });
});
