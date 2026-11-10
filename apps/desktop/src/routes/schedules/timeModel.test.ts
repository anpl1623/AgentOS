import { describe, expect, it } from "vitest";

import { NOW, minutes } from "./schedulesTestData";
import {
  DAY,
  HOUR,
  MINUTE,
  countdown,
  firingLabel,
  fromLocalInput,
  localMoment,
  relative,
  spanOfSeconds,
  toLocalInput,
} from "./timeModel";

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

describe("countdown", () => {
  it("counts in the two largest units", () => {
    expect(countdown(22 * HOUR + 28 * MINUTE)).toBe("22h 28m");
    expect(countdown(6 * DAY + 3 * HOUR + 59 * MINUTE)).toBe("6d 3h");
    expect(countdown(4 * MINUTE + 59_000)).toBe("4m");
  });

  it("rounds down, like every other length on this screen", () => {
    expect(countdown(HOUR - 1)).toBe("59m");
    expect(countdown(DAY - 1)).toBe("23h 59m");
  });

  it("drops a second unit of zero rather than writing it", () => {
    expect(countdown(6 * DAY)).toBe("6d");
    expect(countdown(2 * HOUR + 30_000)).toBe("2h");
  });

  it("does not pretend to count under a minute, or a length it cannot read", () => {
    expect(countdown(59_000)).toBe("<1m");
    expect(countdown(-5 * MINUTE)).toBe("<1m");
    expect(countdown(Number.NaN)).toBe("<1m");
  });
});
