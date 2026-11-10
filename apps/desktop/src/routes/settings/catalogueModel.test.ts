import { describe, expect, it } from "vitest";

import {
  catalogue,
  catalogueSummary,
  countEntries,
  entryMatches,
  filterTerms,
  grantLine,
  holdersOf,
} from "./catalogueModel";
import { agent, tool } from "./testData";

const TOOLS = [
  tool("filesystem.read", "low", { returns_untrusted_data: true }),
  tool("terminal.exec", "high", { returns_untrusted_data: true }),
  tool("filesystem.delete", "high"),
  tool("browser.navigate", "medium", { capabilities: ["browser.navigate"] }),
  tool("filesystem.write", "medium", { capabilities: ["filesystem.write"] }),
];

const AGENTS = [
  agent("sales", ["browser.navigate", "filesystem.write"]),
  agent("ops", ["terminal.exec", "filesystem.read"]),
  agent("archive", ["terminal.exec"], "disabled"),
];

describe("holdersOf", () => {
  it("names every agent granted the tool, disabled ones included, by name", () => {
    expect(holdersOf("terminal.exec", AGENTS)).toEqual([
      { id: "id-archive", name: "archive", enabled: false },
      { id: "id-ops", name: "ops", enabled: true },
    ]);
  });

  it("matches the exact tool name, not a prefix of it", () => {
    const prefixed = [agent("near", ["filesystem.read_all", "filesystem"])];
    expect(holdersOf("filesystem.read", prefixed)).toEqual([]);
  });
});

describe("grantLine", () => {
  it("lists holders, marking the disabled", () => {
    expect(grantLine(holdersOf("terminal.exec", AGENTS))).toEqual({
      text: "granted to archive (disabled), ops",
      nobody: false,
    });
  });

  it("says nobody holds it only when the agents were read", () => {
    expect(grantLine([])).toEqual({ text: "granted to no agent", nobody: true });
    // Unknown is not none: a failed read must not be drawn as "no agent".
    const unknown = grantLine(null);
    expect(unknown.text).not.toContain("no agent");
    expect(unknown.text).toBe("who holds it could not be read");
  });
});

describe("catalogue", () => {
  it("groups by domain alphabetically, most dangerous first within each", () => {
    const groups = catalogue(TOOLS, AGENTS, "");
    expect(groups.map((group) => group.domain)).toEqual(["browser", "filesystem", "terminal"]);
    expect(groups[1]!.entries.map((entry) => entry.tool.name)).toEqual([
      "filesystem.delete",
      "filesystem.write",
      "filesystem.read",
    ]);
    expect(countEntries(groups)).toBe(TOOLS.length);
  });

  it("puts a risk level this build does not know above critical", () => {
    const groups = catalogue(
      [tool("x.known", "critical"), tool("x.future", "catastrophic")],
      [],
      "",
    );
    expect(groups[0]!.entries.map((entry) => entry.tool.name)).toEqual(["x.future", "x.known"]);
  });

  it("falls back to the name's first segment for a tool with no domain", () => {
    const groups = catalogue([tool("mail.send", "high", { domain: "" })], [], "");
    expect(groups[0]!.domain).toBe("mail");
  });

  it("carries holders through, and null holders when the agents are unknown", () => {
    const known = catalogue(TOOLS, AGENTS, "terminal");
    expect(known[0]!.entries[0]!.holders?.map((holder) => holder.name)).toEqual([
      "archive",
      "ops",
    ]);
    const unknown = catalogue(TOOLS, null, "terminal");
    expect(unknown[0]!.entries[0]!.holders).toBeNull();
  });

  it("drops a domain the filter empties", () => {
    const groups = catalogue(TOOLS, AGENTS, "terminal");
    expect(groups.map((group) => group.domain)).toEqual(["terminal"]);
  });

  it("filters by an agent's name, answering what that agent holds", () => {
    const groups = catalogue(TOOLS, AGENTS, "sales");
    expect(groups.flatMap((group) => group.entries.map((entry) => entry.tool.name))).toEqual([
      "browser.navigate",
      "filesystem.write",
    ]);
  });

  it("requires every term to match, case-insensitively", () => {
    const groups = catalogue(TOOLS, AGENTS, "  FILESYSTEM   High ");
    expect(groups.flatMap((group) => group.entries.map((entry) => entry.tool.name))).toEqual([
      "filesystem.delete",
    ]);
  });

  it("returns nothing at all when no tool matches", () => {
    expect(catalogue(TOOLS, AGENTS, "nothing-is-called-this")).toEqual([]);
  });
});

describe("entryMatches", () => {
  it("matches a declared capability, and `external` for a tool that reads untrusted data", () => {
    const entry = {
      tool: tool("filesystem.search", "low", {
        capabilities: ["filesystem.list", "filesystem.read"],
        returns_untrusted_data: true,
      }),
      holders: [],
    };
    expect(entryMatches(entry, filterTerms("filesystem.list"))).toBe(true);
    expect(entryMatches(entry, filterTerms("external"))).toBe(true);
    const internal = { ...entry, tool: { ...entry.tool, returns_untrusted_data: false } };
    expect(entryMatches(internal, filterTerms("external"))).toBe(false);
  });

  it("keeps everything for an empty or blank filter", () => {
    expect(filterTerms("   ")).toEqual([]);
    expect(entryMatches({ tool: tool("a.b", "low"), holders: null }, [])).toBe(true);
  });
});

describe("catalogueSummary", () => {
  it("counts the tools, the dangerous ones and the ones that read untrusted data", () => {
    expect(catalogueSummary(TOOLS)).toEqual({ tools: 5, dangerous: 2, external: 2 });
    expect(catalogueSummary([])).toEqual({ tools: 0, dangerous: 0, external: 0 });
  });

  it("counts critical, and a level this build does not know, as dangerous", () => {
    const summary = catalogueSummary([
      tool("a.merge", "critical"),
      tool("a.future", "catastrophic"),
      tool("a.read", "medium"),
    ]);
    expect(summary.dangerous).toBe(2);
  });
});
