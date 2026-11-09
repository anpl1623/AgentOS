import { describe, expect, it } from "vitest";

import { ICON_NAMES } from "../../components/icons";
import {
  AVATAR_VARIANTS,
  agentStats,
  avatarVariant,
  domainIcon,
  filterAgents,
  highestRisk,
  matchesStatus,
  monogram,
  toolCountLabel,
  toolDomains,
} from "./agentsModel";
import { CATALOGUE, agent, tool } from "./testData";

describe("filterAgents", () => {
  const agents = [
    agent("sales", "anthropic", "claude-opus-5"),
    agent("ops", "ollama", "llama3"),
  ];

  it("keeps everything for a blank query", () => {
    expect(filterAgents(agents, "  ")).toEqual(agents);
  });

  it("matches name, provider and model, ignoring case", () => {
    expect(filterAgents(agents, "SALES").map((a) => a.name)).toEqual(["sales"]);
    expect(filterAgents(agents, "ollama").map((a) => a.name)).toEqual(["ops"]);
    expect(filterAgents(agents, "opus").map((a) => a.name)).toEqual(["sales"]);
  });

  it("matches provider and model as the row writes them", () => {
    expect(filterAgents(agents, "ollama/llama").map((a) => a.name)).toEqual(["ops"]);
  });

  it("finds nothing for a query nothing contains", () => {
    expect(filterAgents(agents, "gemini")).toEqual([]);
  });
});

describe("matchesStatus", () => {
  const on = agent("sales", "anthropic", "claude-opus-5");
  const off = { ...agent("ops", "ollama", "llama3"), status: "disabled" };
  const odd = { ...agent("odd", "mock", "mock"), status: "paused" };

  it("keeps every agent for all", () => {
    expect([on, off, odd].filter((a) => matchesStatus(a, "all"))).toHaveLength(3);
  });

  it("splits on whether the agent may be given work", () => {
    expect([on, off].filter((a) => matchesStatus(a, "enabled")).map((a) => a.name)).toEqual([
      "sales",
    ]);
    expect([on, off].filter((a) => matchesStatus(a, "disabled")).map((a) => a.name)).toEqual([
      "ops",
    ]);
  });

  it("counts a status this build does not know as disabled, as the chip does", () => {
    expect(matchesStatus(odd, "disabled")).toBe(true);
    expect(matchesStatus(odd, "enabled")).toBe(false);
  });
});

describe("highestRisk", () => {
  it("is the most dangerous granted risk, so terminal.exec shows from the list", () => {
    expect(highestRisk(["filesystem.read", "terminal.exec"], CATALOGUE)).toBe("high");
  });

  it("ignores names the catalogue does not know", () => {
    expect(highestRisk(["filesystem.read", "nothing.here"], CATALOGUE)).toBe("low");
    expect(highestRisk(["nothing.here"], CATALOGUE)).toBeNull();
    expect(highestRisk([], CATALOGUE)).toBeNull();
  });

  it("ranks a level this build does not know above critical", () => {
    const catalogue = [tool("a.b", "critical"), tool("c.d", "catastrophic")];
    expect(highestRisk(["a.b", "c.d"], catalogue)).toBe("catastrophic");
  });
});

describe("agentStats", () => {
  const agents = [
    agent("sales", "anthropic", "claude-opus-5", ["filesystem.read", "terminal.exec"]),
    { ...agent("ops", "ollama", "llama3", ["filesystem.read"]), status: "disabled" },
    agent("quiet", "mock", "mock"),
  ];

  it("counts enabled, disabled, distinct tools and providers", () => {
    expect(agentStats(agents, CATALOGUE)).toEqual({
      total: 3,
      enabled: 2,
      disabled: 1,
      highRisk: 1,
      toolsInUse: 2,
      providers: 3,
    });
  });

  it("does not claim a count of high-risk agents before the catalogue answers", () => {
    expect(agentStats(agents, null).highRisk).toBeNull();
  });

  it("counts critical and unknown levels as high risk", () => {
    const catalogue = [tool("a.b", "critical"), tool("c.d", "catastrophic"), tool("e.f", "low")];
    const held = [
      agent("one", "mock", "mock", ["a.b"]),
      agent("two", "mock", "mock", ["c.d"]),
      agent("three", "mock", "mock", ["e.f"]),
    ];
    expect(agentStats(held, catalogue).highRisk).toBe(2);
  });

  it("is all zeroes for no agents", () => {
    expect(agentStats([], CATALOGUE)).toEqual({
      total: 0,
      enabled: 0,
      disabled: 0,
      highRisk: 0,
      toolsInUse: 0,
      providers: 0,
    });
  });
});

describe("toolCountLabel", () => {
  it("words the count", () => {
    expect([0, 1, 3].map(toolCountLabel)).toEqual(["No tools", "1 tool", "3 tools"]);
  });
});

describe("toolDomains", () => {
  it("lists each domain once, in the order first granted", () => {
    expect(
      toolDomains(["browser.navigate", "filesystem.write", "browser.extract", "github.pulls.get"]),
    ).toEqual(["browser", "filesystem", "github"]);
  });

  it("keeps a name with no dot as its own domain", () => {
    expect(toolDomains(["delegate", "agent.delegate"])).toEqual(["delegate", "agent"]);
    expect(toolDomains([])).toEqual([]);
  });
});

describe("domainIcon", () => {
  it("draws every known domain with an icon the set has", () => {
    for (const domain of ["browser", "network", "filesystem", "terminal", "github", "agent"]) {
      expect(ICON_NAMES).toContain(domainIcon(domain));
    }
  });

  it("gives a domain it does not know the generic tool mark", () => {
    expect(domainIcon("teleport")).toBe("zap");
  });
});

describe("monogram", () => {
  it("is the first letter of a one-word name", () => {
    expect(monogram("sales")).toBe("S");
  });

  it("is the initials of the first two words", () => {
    expect(monogram("web-researcher")).toBe("WR");
    expect(monogram("ops_bot two")).toBe("OB");
    expect(monogram("sales.eu")).toBe("SE");
  });

  it("does not split a letter outside the basic plane", () => {
    expect(monogram("🦊 fox")).toBe("🦊F");
  });

  it("says something for a name with no letters to take", () => {
    expect(monogram("")).toBe("?");
    expect(monogram("--")).toBe("?");
  });
});

describe("avatarVariant", () => {
  it("is the same for the same name, every time", () => {
    expect(avatarVariant("sales")).toBe(avatarVariant("sales"));
  });

  it("stays within the variants the stylesheet defines", () => {
    for (const name of ["sales", "ops", "supervisor", "researcher", "writer", "reader", ""]) {
      const variant = avatarVariant(name);
      expect(Number.isInteger(variant)).toBe(true);
      expect(variant).toBeGreaterThanOrEqual(0);
      expect(variant).toBeLessThan(AVATAR_VARIANTS);
    }
  });

  it("gives different names different faces often enough to tell them apart", () => {
    const names = ["sales", "ops", "supervisor", "researcher", "writer", "reader"];
    expect(new Set(names.map(avatarVariant)).size).toBeGreaterThan(1);
  });
});
