import { describe, expect, it } from "vitest";

import { asSentence, counted, relative } from "./settingsModel";
import { NOW, minutesFrom } from "./testData";

describe("relative", () => {
  it("says which side of now a moment falls, in whole units rounded down", () => {
    expect(relative(minutesFrom(11.9), NOW)).toBe("in 11m");
    expect(relative(minutesFrom(-11.9), NOW)).toBe("11m ago");
    expect(relative(minutesFrom(150), NOW)).toBe("in 2h");
    expect(relative(minutesFrom(-3 * 24 * 60), NOW)).toBe("3d ago");
  });

  it("calls anything within a minute now, and says so of a time it cannot read", () => {
    expect(relative(minutesFrom(0.5), NOW)).toBe("now");
    expect(relative(minutesFrom(-0.5), NOW)).toBe("now");
    expect(relative("not a time", NOW)).toBe("at an unreadable time");
  });
});

describe("asSentence", () => {
  it("capitalises and closes a runtime message without changing its words", () => {
    expect(asSentence("a tick shorter than 5 seconds is a busy loop")).toBe(
      "A tick shorter than 5 seconds is a busy loop.",
    );
    expect(asSentence("  already a sentence.  ")).toBe("Already a sentence.");
    expect(asSentence("")).toBe("");
  });
});

describe("counted", () => {
  it("is singular for one and plural for any other count, none included", () => {
    expect(counted(1, "break", "breaks")).toBe("1 break");
    expect(counted(2, "break", "breaks")).toBe("2 breaks");
    expect(counted(0, "break", "breaks")).toBe("0 breaks");
  });
});
