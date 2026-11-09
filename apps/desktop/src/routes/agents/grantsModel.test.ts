import { describe, expect, it } from "vitest";

import {
  REACH_LEGEND,
  aboveCeiling,
  deadGrantText,
  grantRows,
  grantTally,
  reachLabel,
  reachTone,
  refusedFigure,
} from "./grantsModel";
import { CATALOGUE, grant, policy, tool } from "./testData";

describe("grantRows", () => {
  it("draws risk, external and the capabilities each tool declares", () => {
    const [row] = grantRows(["terminal.exec"], CATALOGUE, null, policy());
    expect(row).toMatchObject({
      tool: "terminal.exec",
      risk: "high",
      external: true,
      description: "does terminal.exec",
      capabilities: [{ capability: "terminal.exec", reach: null }],
      dead: null,
    });
  });

  it("takes each capability's reach from the engine's report", () => {
    const report = [
      grant("filesystem.search", { "filesystem.list": "allowed", "filesystem.read": "scoped" }),
    ];
    const [row] = grantRows(["filesystem.search"], CATALOGUE, report, policy());
    expect(row?.capabilities).toEqual([
      { capability: "filesystem.list", reach: "allowed" },
      { capability: "filesystem.read", reach: "scoped" },
    ]);
    expect(row?.dead).toBeNull();
  });

  it("marks a tool whose every capability the engine denies", () => {
    const report = [grant("filesystem.write", { "filesystem.write": "denied" })];
    expect(grantRows(["filesystem.write"], CATALOGUE, report, policy())[0]?.dead).toBe("no-rule");
  });

  it("does not mark a tool with one capability denied and another reaching", () => {
    const report = [
      grant("filesystem.search", { "filesystem.list": "denied", "filesystem.read": "asks" }),
    ];
    expect(grantRows(["filesystem.search"], CATALOGUE, report, policy())[0]?.dead).toBeNull();
  });

  it("claims nothing about rules before the report has answered", () => {
    expect(grantRows(["filesystem.write"], CATALOGUE, null, policy())[0]?.dead).toBeNull();
  });

  it("does not call a tool that declares no capability dead", () => {
    const report = [grant("odd.tool", {})];
    const catalogue = [tool("odd.tool", "low", [])];
    expect(grantRows(["odd.tool"], catalogue, report, policy())[0]?.dead).toBeNull();
  });

  it("marks a name the registry does not know, from either source", () => {
    expect(grantRows(["gone.tool"], CATALOGUE, null, policy())[0]?.dead).toBe("unknown");
    const report = [grant("gone.tool", {}, false)];
    expect(grantRows(["gone.tool"], null, report, policy())[0]?.dead).toBe("unknown");
  });

  it("waits for a source before calling a name unknown", () => {
    expect(grantRows(["gone.tool"], null, null, policy())[0]?.dead).toBeNull();
  });

  it("names the ceiling rather than a missing rule for a tool above it", () => {
    // The engine reports the tool as denied too; the ceiling is the reason a
    // person can act on, and adding a rule would not help.
    const report = [grant("terminal.exec", { "terminal.exec": "denied" })];
    const rows = grantRows(["terminal.exec"], CATALOGUE, report, policy({ max_risk: "medium" }));
    expect(rows[0]?.dead).toBe("ceiling");
  });

  it("does not mark a tool at the ceiling", () => {
    const rows = grantRows(["filesystem.write"], CATALOGUE, null, policy({ max_risk: "medium" }));
    expect(rows[0]?.dead).toBeNull();
  });

  it("keeps the granted order and one row per grant", () => {
    const rows = grantRows(["terminal.exec", "filesystem.read"], CATALOGUE, null, null);
    expect(rows.map((row) => row.tool)).toEqual(["terminal.exec", "filesystem.read"]);
  });

  it("takes the domain from the catalogue, or from the name when the catalogue lacks it", () => {
    const catalogue = [{ ...tool("gh.pulls.merge", "critical"), domain: "github" }];
    const rows = grantRows(["gh.pulls.merge", "agent.delegate", "solo"], catalogue, null, null);
    expect(rows.map((row) => row.domain)).toEqual(["github", "agent", "solo"]);
  });
});

describe("aboveCeiling", () => {
  it("compares known levels strictly", () => {
    expect(aboveCeiling("high", "medium")).toBe(true);
    expect(aboveCeiling("medium", "medium")).toBe(false);
    expect(aboveCeiling("low", "medium")).toBe(false);
    expect(aboveCeiling("critical", null)).toBe(false);
  });

  it("claims nothing for a level this build does not know", () => {
    expect(aboveCeiling("catastrophic", "high")).toBe(false);
    expect(aboveCeiling("high", "someday")).toBe(false);
  });
});

describe("deadGrantText", () => {
  const base = grantRows(["filesystem.write"], CATALOGUE, null, policy())[0]!;

  it("says nothing for a live grant", () => {
    expect(deadGrantText(base, policy())).toBeNull();
  });

  it("says every call will be refused when no rule allows the tool", () => {
    const text = deadGrantText({ ...base, dead: "no-rule" }, policy());
    expect(text?.label).toBe("No rule allows this");
    expect(text?.sentence).toBe("No rule allows this — every call will be refused.");
  });

  it("names both levels for the ceiling", () => {
    const row = { ...base, risk: "high", dead: "ceiling" as const };
    const text = deadGrantText(row, policy({ max_risk: "medium" }));
    expect(text?.label).toBe("Above the risk ceiling");
    expect(text?.sentence).toContain("high risk is above this policy's ceiling of medium");
    expect(text?.sentence).toContain("every call will be refused");
  });

  it("calls an unregistered name an unknown tool", () => {
    expect(deadGrantText({ ...base, dead: "unknown" }, null)?.label).toBe("Unknown tool");
  });
});

describe("reach", () => {
  it("labels and tones the engine's four answers", () => {
    expect(["allowed", "scoped", "asks", "denied"].map(reachLabel)).toEqual([
      "Allowed",
      "Scoped",
      "Asks",
      "Denied",
    ]);
    expect(["allowed", "scoped", "asks", "denied"].map(reachTone)).toEqual([
      "ok",
      "neutral",
      "warn",
      "blocked",
    ]);
  });

  it("passes an unknown reach through as written, in a neutral tone", () => {
    expect(reachLabel("partial")).toBe("partial");
    expect(reachTone("partial")).toBe("neutral");
  });

  it("explains each of the four answers in the key, and only those", () => {
    expect(REACH_LEGEND.map((entry) => entry.reach)).toEqual([
      "allowed",
      "scoped",
      "asks",
      "denied",
    ]);
    // Every entry is a reach the chips know, so the key and the rows agree.
    for (const { reach, meaning } of REACH_LEGEND) {
      expect(reachLabel(reach)).not.toBe(reach);
      expect(meaning.trim()).not.toBe("");
    }
  });
});

describe("grantTally", () => {
  it("counts grants, refusals and the highest risk the catalogue knows", () => {
    const report = [grant("filesystem.write", { "filesystem.write": "denied" })];
    const rows = grantRows(
      ["filesystem.read", "filesystem.write", "gone.tool"],
      CATALOGUE,
      report,
      policy(),
    );
    expect(grantTally(rows)).toEqual({ granted: 3, refused: 2, highest: "medium" });
  });

  it("is empty for no grants", () => {
    expect(grantTally([])).toEqual({ granted: 0, refused: 0, highest: null });
  });
});

describe("refusedFigure", () => {
  const allowed = grant("filesystem.read", { "filesystem.read": "allowed" });

  it("says none only once the report has been read", () => {
    const rows = grantRows(["filesystem.read"], CATALOGUE, [allowed], policy());
    expect(refusedFigure(rows, "read")).toEqual({ value: 0, hint: "none found", tone: "accent" });
  });

  it("claims nothing while the report is still being read", () => {
    const rows = grantRows(["filesystem.read"], CATALOGUE, null, policy());
    expect(refusedFigure(rows, "reading")).toEqual({
      value: null,
      hint: "reading the policy…",
      tone: "accent",
    });
  });

  it("does not read a failed report as a clean one", () => {
    const rows = grantRows(["filesystem.read"], CATALOGUE, null, policy());
    const figure = refusedFigure(rows, "failed");
    expect(figure.value).toBeNull();
    expect(figure.tone).toBe("warn");
    expect(figure.hint).toContain("could not be read");
  });

  it("counts refusals the catalogue establishes without the report", () => {
    const rows = grantRows(["gone.tool", "filesystem.read"], CATALOGUE, null, policy());
    expect(refusedFigure(rows, "reading")).toMatchObject({ value: 1, tone: "danger" });
    expect(refusedFigure(rows, "failed")).toMatchObject({ value: 1, tone: "danger" });
    expect(refusedFigure(rows, "failed").hint).toContain("could not be read");
  });

  it("has nothing to refuse when nothing is granted", () => {
    expect(refusedFigure([], "reading")).toEqual({
      value: 0,
      hint: "no tools granted",
      tone: "accent",
    });
  });
});
