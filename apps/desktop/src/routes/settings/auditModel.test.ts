import { describe, expect, it } from "vitest";

import {
  auditFindings,
  findingsSummary,
  gapsNote,
  verificationAnnouncement,
  verificationLine,
  verificationTone,
  verificationWord,
} from "./auditModel";
import { NOW, minutesFrom } from "./testData";

describe("verification", () => {
  it("says the chain has not been checked rather than leaving a blank", () => {
    expect(verificationLine(null, NOW)).toBe("not verified since the window opened");
  });

  it("says when it was checked and what was found", () => {
    expect(verificationLine(auditFindings(minutesFrom(-4), { breaks: [], gaps: [] }), NOW)).toBe(
      "verified 4m ago · intact",
    );
    expect(
      verificationLine(auditFindings(minutesFrom(-4), { breaks: ["a", "b"], gaps: [] }), NOW),
    ).toBe("verified 4m ago · 2 breaks");
    expect(
      verificationLine(auditFindings(minutesFrom(0), { breaks: ["a"], gaps: ["g"] }), NOW),
    ).toBe("verified now · 1 break, 1 coverage gap");
  });

  it("never calls a chain intact while the log it holds has a gap", () => {
    const findings = auditFindings(minutesFrom(-1), {
      breaks: [],
      gaps: ["run abc: a tool execution started and never finished"],
    });
    expect(findings.breaks).toEqual([]);
    expect(findings.gaps).toEqual(["run abc: a tool execution started and never finished"]);
    expect(findings.problems).toEqual(["run abc: a tool execution started and never finished"]);
    expect(verificationLine(findings, NOW)).toBe("verified 1m ago · 1 coverage gap");
  });

  it("counts breaks and gaps together, breaks first, and keeps them apart for listing", () => {
    const findings = auditFindings(minutesFrom(0), { breaks: ["b"], gaps: ["g1", "g2"] });
    expect(findings.problems).toEqual(["b", "g1", "g2"]);
    expect([findings.breaks, findings.gaps]).toEqual([["b"], ["g1", "g2"]]);
    expect(verificationLine(auditFindings(minutesFrom(0), { breaks: [], gaps: [] }), NOW)).toBe(
      "verified now · intact",
    );
  });

  it("announces the result as a sentence", () => {
    expect(verificationAnnouncement({ breaks: [], gaps: [] })).toBe("Audit chain verified intact.");
    expect(verificationAnnouncement({ breaks: ["a"], gaps: [] })).toBe(
      "Audit chain verification found 1 break.",
    );
    expect(verificationAnnouncement({ breaks: ["a", "b"], gaps: ["g"] })).toBe(
      "Audit chain verification found 2 breaks, and 1 coverage gap.",
    );
  });

  it("never announces a log with only a gap as a chain that failed", () => {
    const sentence = verificationAnnouncement({ breaks: [], gaps: ["g1", "g2"] });
    expect(sentence).toBe("Audit chain verified, but its records have 2 coverage gaps.");
    expect(sentence).not.toMatch(/verification found|break|problem/);
    expect(findingsSummary({ breaks: [], gaps: ["g"] })).toBe("1 coverage gap");
  });

  it("says the chain verifies above the gaps only when no break is listed", () => {
    expect(gapsNote({ breaks: [], gaps: ["g"] })).toMatch(/^The chain verifies/);
    const both = gapsNote({ breaks: ["b"], gaps: ["g"] });
    expect(both).not.toMatch(/chain verifies/);
    expect(both).toMatch(/Separately/);
  });
});

describe("verificationTone", () => {
  it("draws a break as a failure, gaps alone as attention, and only a clean chain as well", () => {
    expect(verificationTone({ breaks: ["b"], gaps: [] })).toBe("danger");
    expect(verificationTone({ breaks: ["b"], gaps: ["g"] })).toBe("danger");
    expect(verificationTone({ breaks: [], gaps: ["g"] })).toBe("warn");
    expect(verificationTone({ breaks: [], gaps: [] })).toBe("ok");
  });
});

describe("verificationWord", () => {
  it("says a chain never checked is unverified, not intact", () => {
    expect(verificationWord(null)).toBe("Unverified");
    expect(verificationWord({ breaks: [], gaps: [] })).toBe("Intact");
  });

  it("names breaks before gaps, and never calls a gap a break", () => {
    expect(verificationWord({ breaks: ["b"], gaps: ["g1", "g2"] })).toBe("1 break");
    expect(verificationWord({ breaks: [], gaps: ["g1", "g2"] })).toBe("2 gaps");
    expect(verificationWord({ breaks: [], gaps: ["g"] })).toBe("1 gap");
  });
});
