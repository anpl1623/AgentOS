import { describe, expect, it } from "vitest";

import {
  CRON_PRESETS,
  INTERVAL_PRESETS,
  cadenceIcon,
  cadenceWords,
  cronWords,
  describeCron,
  intervalWords,
  previewWords,
} from "./cadenceModel";
import { CADENCE_KINDS, cadenceVerdict } from "./formModel";
import { NOW, cadence, minutes, preview } from "./schedulesTestData";

describe("intervalWords", () => {
  it("says an interval in the largest unit that divides it exactly", () => {
    expect(intervalWords(3_600)).toBe("Every hour");
    expect(intervalWords(21_600)).toBe("Every 6 hours");
    expect(intervalWords(5_400)).toBe("Every 90 minutes");
    expect(intervalWords(86_400)).toBe("Every day");
    expect(intervalWords(604_800 * 2)).toBe("Every 2 weeks");
    expect(intervalWords(90)).toBe("Every 90 seconds");
  });

  it("has no words for a value that is not a positive whole number of seconds", () => {
    expect(intervalWords(0)).toBeNull();
    expect(intervalWords(-60)).toBeNull();
    expect(intervalWords(1.5)).toBeNull();
  });
});

describe("describeCron", () => {
  const words = (expression: string) => describeCron(expression)?.words ?? null;

  it("counts the days of the week from Sunday as 1, as the runtime does", () => {
    // The trap: in crontab `1-5` is Monday to Friday; here it is not.
    expect(words("0 9 * * 1-5")).toBe("Sun to Thu at 09:00");
    expect(words("0 9 * * 2-6")).toBe("Weekdays at 09:00");
    expect(words("0 9 * * MON-FRI")).toBe("Weekdays at 09:00");
    expect(words("0 9 * * mon-fri")).toBe("Weekdays at 09:00");
  });

  it("names single days, weekends, lists and runs", () => {
    expect(words("30 8 * * MON")).toBe("Mondays at 08:30");
    expect(words("0 10 * * SAT,SUN")).toBe("Weekends at 10:00");
    expect(words("0 9 * * MON,WED,FRI")).toBe("Mon, Wed and Fri at 09:00");
    expect(words("0 9 * * TUE,THU")).toBe("Tue and Thu at 09:00");
    expect(words("0 9 * * 1-7")).toBe("Every day at 09:00");
  });

  it("says every day, several times a day, monthly and yearly", () => {
    expect(words("0 9 * * *")).toBe("Every day at 09:00");
    expect(words("0 9,17 * * *")).toBe("Every day at 09:00 and 17:00");
    expect(words("0 9 1 * *")).toBe("Monthly on the 1st at 09:00");
    expect(words("0 9 22 * *")).toBe("Monthly on the 22nd at 09:00");
    expect(words("0 9 11 * *")).toBe("Monthly on the 11th at 09:00");
    expect(words("0 0 1 JAN *")).toBe("Every year on 1 Jan at 00:00");
  });

  it("says the hourly and minutely shapes", () => {
    expect(words("* * * * *")).toBe("Every minute");
    expect(words("*/15 * * * *")).toBe("Every 15 minutes");
    expect(words("0/5 * * * *")).toBe("Every 5 minutes");
    expect(words("0 * * * *")).toBe("Every hour, on the hour");
    expect(words("30 * * * *")).toBe("Every hour at :30");
    expect(words("0 */2 * * *")).toBe("Every 2 hours, on the hour");
  });

  it("says no step that would be wrong where the hour or the day wraps", () => {
    // `*/7` fires at :56 and then at :00, four minutes later; `*/5` hours fires
    // at 20:00 and then at midnight, four hours later.
    expect(words("*/7 * * * *")).toBeNull();
    expect(words("*/45 * * * *")).toBeNull();
    expect(words("0 */5 * * *")).toBeNull();
    expect(words("0 */7 * * *")).toBeNull();
    expect(words("*/20 * * * *")).toBe("Every 20 minutes");
    expect(words("0 */6 * * *")).toBe("Every 6 hours, on the hour");
  });

  it("reads a six-field expression with seconds first, and a seventh field of any year", () => {
    expect(words("0 0 9 * * MON-FRI")).toBe("Weekdays at 09:00");
    expect(words("0 0 9 * * * *")).toBe("Every day at 09:00");
  });

  it("says nothing rather than something wrong about what it does not understand", () => {
    for (const expression of [
      "30 0 9 * * *", // a seconds field other than 0
      "0 0 1 1 * 2027", // six fields read seconds-first: an hour of 1 on day-of-week 2027
      "0 0 9 * * * 2027", // a year
      "0 9 1 * MON", // day of month and day of week together
      "0 9 * * 0", // Sunday as 0, which the runtime refuses
      "0 9 * * 1/2", // a step over days
      "0 9-17 * * *", // a range of hours
      "0 1,2,3,4,5 * * *", // more hours than reads well
      "61 9 * * *",
      "not cron at all",
      "",
    ]) {
      expect(describeCron(expression), expression).toBeNull();
    }
  });
});

describe("cronWords", () => {
  it("says UTC for a time of day read in UTC, and nothing for local time", () => {
    expect(cronWords("0 9 * * MON-FRI", "utc")).toBe("Weekdays at 09:00 UTC");
    expect(cronWords("0 9 * * MON-FRI", "local")).toBe("Weekdays at 09:00");
  });

  it("does not add a clock to a shape no clock changes", () => {
    expect(cronWords("*/15 * * * *", "utc")).toBe("Every 15 minutes");
  });

  it("says UTC for a minute past the hour read in UTC, which a half-hour offset moves", () => {
    expect(cronWords("30 * * * *", "utc")).toBe("Every hour at :30 UTC");
    expect(cronWords("0 */2 * * *", "utc")).toBe("Every 2 hours, on the hour UTC");
    expect(cronWords("30 * * * *", "local")).toBe("Every hour at :30");
  });
});

describe("cadenceWords", () => {
  it("speaks for each kind, and for no kind it does not know", () => {
    const shape = { seconds: null, expression: null, clock: null };
    expect(cadenceWords({ ...shape, kind: "every", seconds: 21_600 })).toBe("Every 6 hours");
    expect(cadenceWords({ ...shape, kind: "cron", expression: "0 9 * * *", clock: "utc" })).toBe(
      "Every day at 09:00 UTC",
    );
    expect(cadenceWords({ ...shape, kind: "once" })).toBe("Once");
    expect(cadenceWords({ ...shape, kind: "lunar" })).toBeNull();
    expect(cadenceWords({ ...shape, kind: "cron" })).toBeNull();
  });
});

describe("cadenceIcon", () => {
  it("gives each kind a mark of its own, and an unknown kind the screen's", () => {
    const marks = CADENCE_KINDS.map(({ kind }) => cadenceIcon(kind));
    expect(new Set(marks).size).toBe(CADENCE_KINDS.length);
    expect(cadenceIcon("lunar")).toBe("schedules");
  });
});

describe("presets", () => {
  it("labels every preset with its own words, so a label cannot disagree with what it fills in", () => {
    for (const preset of CRON_PRESETS) {
      expect(preset.words).toBe(cronWords(preset.expression, "local"));
    }
    for (const preset of INTERVAL_PRESETS) {
      expect(preset.words).toBe(intervalWords(preset.seconds));
    }
  });

  it("offers weekdays by name first, which the runtime cannot misread", () => {
    expect(CRON_PRESETS[0]).toEqual({ expression: "0 9 * * MON-FRI", words: "Weekdays at 09:00" });
  });
});

describe("previewWords", () => {
  it("says the cadence in words over the runtime's own description", () => {
    const typed = cadence({ kind: "every", seconds: "21600" });
    const verdict = cadenceVerdict(
      typed,
      NOW,
      preview([minutes(360)], { description: "every 21600s" }),
      null,
    );
    expect(previewWords(typed, verdict)).toEqual({
      words: "Every 6 hours",
      exact: "every 21600s",
    });
  });

  it("falls back to the runtime's description when it has no words of its own", () => {
    const typed = cadence({ kind: "cron", expression: "0 9-17 * * *" });
    const verdict = cadenceVerdict(
      typed,
      NOW,
      preview([minutes(60)], { description: "cron `0 9-17 * * *` (local)" }),
      null,
    );
    expect(previewWords(typed, verdict)).toEqual({
      words: "cron `0 9-17 * * *` (local)",
      exact: null,
    });
  });

  it("says a one-shot's moment, and nothing while there is no valid preview", () => {
    const once = cadence({ kind: "once", at: "2026-10-09T08:30" });
    expect(previewWords(once, cadenceVerdict(once, NOW, null, null))).toEqual({
      words: "Once, in 2d",
      exact: null,
    });
    expect(previewWords(cadence(), { state: "checking" })).toBeNull();
  });
});
