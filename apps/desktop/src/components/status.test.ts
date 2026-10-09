import { describe, expect, it } from "vitest";

import { RISK_ORDER, compareRisk, outcomeTone, riskTone, runTone, taskTone } from "./status";

describe("compareRisk", () => {
  it("orders the runtime's levels from none to critical", () => {
    const shuffled = ["high", "none", "critical", "low", "medium"];
    expect([...shuffled].sort(compareRisk)).toEqual([...RISK_ORDER]);
  });

  it("treats equal levels as equal", () => {
    for (const level of RISK_ORDER) expect(compareRisk(level, level)).toBe(0);
  });

  it("ranks an unrecognised level above critical, so it is not buried", () => {
    expect(compareRisk("catastrophic", "critical")).toBeGreaterThan(0);
    expect(compareRisk("none", "")).toBeLessThan(0);
    const descending = ["low", "catastrophic", "critical"].sort((a, b) => compareRisk(b, a));
    expect(descending[0]).toBe("catastrophic");
  });

  it("does not mistake prototype names for levels", () => {
    expect(compareRisk("constructor", "critical")).toBeGreaterThan(0);
  });
});

describe("outcomeTone", () => {
  it("maps every ToolOutcome wire value explicitly", () => {
    expect(outcomeTone("success")).toBe("ok");
    expect(outcomeTone("failed")).toBe("danger");
    expect(outcomeTone("timed_out")).toBe("danger");
    expect(outcomeTone("invalid_arguments")).toBe("danger");
    expect(outcomeTone("denied")).toBe("blocked");
    expect(outcomeTone("approval_denied")).toBe("blocked");
    expect(outcomeTone("cancelled")).toBe("neutral");
  });

  it("never shows a refusal in the colour of a failure or a success", () => {
    for (const refusal of ["denied", "approval_denied"]) {
      expect(outcomeTone(refusal)).not.toBe("danger");
      expect(outcomeTone(refusal)).not.toBe("ok");
    }
  });

  it("falls back to neutral for anything unknown, including prototype keys", () => {
    for (const value of ["", "exploded", "constructor", "toString", "__proto__"]) {
      expect(outcomeTone(value)).toBe("neutral");
      expect(runTone(value)).toBe("neutral");
      expect(taskTone(value)).toBe("neutral");
    }
  });
});

describe("runTone and taskTone", () => {
  it("keep the accent tone for work still in flight", () => {
    for (const state of ["planning", "executing", "waiting_for_approval", "recovering"]) {
      expect(runTone(state)).toBe("live");
    }
    expect(taskTone("running")).toBe("live");
  });

  it("do not paint a task waiting on its graph as a policy refusal", () => {
    expect(taskTone("blocked")).toBe("neutral");
  });
});

describe("riskTone", () => {
  it("draws each known level as itself", () => {
    for (const level of RISK_ORDER) expect(riskTone(level)).toBe(level);
  });

  it("draws an unknown level as critical, as compareRisk sorts it", () => {
    // The row sorted to the front as most dangerous wore the quietest chip.
    expect(riskTone("catastrophic")).toBe("critical");
    expect(riskTone("constructor")).toBe("critical");
    expect(riskTone("")).toBe("critical");
    const highest = ["critical", "catastrophic"].sort(compareRisk).at(-1) ?? "";
    expect(riskTone(highest)).toBe(riskTone("critical"));
  });
});
