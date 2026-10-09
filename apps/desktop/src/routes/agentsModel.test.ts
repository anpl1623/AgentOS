import { describe, expect, it } from "vitest";

import type { AgentSummary } from "../bindings/AgentSummary";
import type { MemoryView } from "../bindings/MemoryView";
import type { PolicyView } from "../bindings/PolicyView";
import type { ToolGrantView } from "../bindings/ToolGrantView";
import type { ToolView } from "../bindings/ToolView";
import {
  aboveCeiling,
  beginPolicyDraft,
  confidenceLabel,
  createFormDirty,
  createInput,
  deadGrantText,
  filterAgents,
  filterMemories,
  grantRows,
  highestRisk,
  memoryRevision,
  newCreateForm,
  policyDirty,
  policySuperseded,
  reachLabel,
  reachTone,
  splitRule,
  toggleTool,
} from "./agentsModel";

function agent(name: string, provider: string, model: string, tools: string[] = []): AgentSummary {
  return {
    id: `id-${name}`,
    name,
    provider,
    model,
    status: "enabled",
    tools,
    max_steps: 24,
    created_at: "2026-10-01T00:00:00Z",
  };
}

function tool(
  name: string,
  risk: string,
  capabilities: string[] = [name],
  untrusted = false,
): ToolView {
  return {
    name,
    domain: name.split(".")[0] ?? name,
    description: `does ${name}`,
    risk,
    returns_untrusted_data: untrusted,
    capabilities,
  };
}

function grant(name: string, reaches: Record<string, string>, registered = true): ToolGrantView {
  const capabilities = Object.entries(reaches).map(([capability, reach]) => ({
    capability,
    reach,
  }));
  return { tool: name, registered, reach: "denied", capabilities };
}

function policy(extra: Partial<PolicyView> = {}): PolicyView {
  return {
    document: "default: deny\n",
    version: 3,
    default_effect: "deny",
    max_risk: null,
    taint_enabled: true,
    taint_threshold: "high",
    rules: [],
    ...extra,
  };
}

function memory(id: string, extra: Partial<MemoryView> = {}): MemoryView {
  return {
    id,
    agent_id: "id-sales",
    kind: "fact",
    content: "The CRM is at port 8420.",
    source: "user",
    source_untrusted: false,
    reaches_the_prompt: true,
    confidence: 1,
    task_id: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...extra,
  };
}

const CATALOGUE = [
  tool("filesystem.read", "low"),
  tool("filesystem.write", "medium"),
  tool("terminal.exec", "high", ["terminal.exec"], true),
  tool("filesystem.search", "low", ["filesystem.list", "filesystem.read"]),
];

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
});

describe("splitRule", () => {
  it("splits on the first separator", () => {
    expect(splitRule("browser.navigate => allow on [origin:http://127.0.0.1:8420]")).toEqual({
      capability: "browser.navigate",
      effect: "allow on [origin:http://127.0.0.1:8420]",
    });
    expect(splitRule("a.b => ask on [x => y]")).toEqual({
      capability: "a.b",
      effect: "ask on [x => y]",
    });
  });

  it("keeps a line without one whole rather than dropping it", () => {
    expect(splitRule("something the engine said")).toEqual({
      capability: "something the engine said",
      effect: "",
    });
  });
});

describe("policy drafts", () => {
  it("is clean until the text changes, and clean again when changed back", () => {
    const draft = beginPolicyDraft(policy());
    expect(policyDirty(draft)).toBe(false);
    expect(policyDirty({ ...draft, document: "default: allow\n" })).toBe(true);
    expect(policyDirty({ ...draft, document: "default: deny\n" })).toBe(false);
    expect(policyDirty(null)).toBe(false);
  });

  it("notices a save made elsewhere after editing began", () => {
    const draft = beginPolicyDraft(policy({ version: 3 }));
    expect(policySuperseded(draft, policy({ version: 3 }))).toBe(false);
    expect(policySuperseded(draft, policy({ version: 4 }))).toBe(true);
    expect(policySuperseded(null, policy())).toBe(false);
    expect(policySuperseded(draft, null)).toBe(false);
  });
});

describe("create form", () => {
  it("opens clean", () => {
    expect(createFormDirty(newCreateForm())).toBe(false);
    expect(createFormDirty(null)).toBe(false);
  });

  it("is dirty after any field changes", () => {
    const form = newCreateForm();
    expect(createFormDirty({ ...form, name: "sales" })).toBe(true);
    expect(createFormDirty({ ...form, vision: "yes" })).toBe(true);
    expect(createFormDirty({ ...form, tools: toggleTool(form.tools, "terminal.exec") })).toBe(true);
  });

  it("compares tools as a set", () => {
    const form = newCreateForm();
    const again = toggleTool(toggleTool(form.tools, "filesystem.read"), "filesystem.read");
    expect(again).not.toEqual(form.tools);
    expect(createFormDirty({ ...form, tools: again })).toBe(false);
  });

  it("sends a base URL only for a provider that takes one", () => {
    const form = { ...newCreateForm(), name: " sales ", baseUrl: " http://localhost:11434/v1 " };
    expect(createInput({ ...form, provider: "ollama" }).base_url).toBe("http://localhost:11434/v1");
    expect(createInput({ ...form, provider: "anthropic" }).base_url).toBeNull();
    expect(createInput({ ...form, provider: "openai", baseUrl: "  " }).base_url).toBeNull();
    expect(createInput(form).name).toBe("sales");
  });

  it("maps vision's three states", () => {
    const form = newCreateForm();
    expect(createInput(form).vision).toBeNull();
    expect(createInput({ ...form, vision: "yes" }).vision).toBe(true);
    expect(createInput({ ...form, vision: "no" }).vision).toBe(false);
  });
});

describe("filterMemories", () => {
  const memories = [
    memory("1", { kind: "preference", content: "Never send without approval." }),
    memory("2", {
      kind: "observation",
      content: "Globex wants exports sent elsewhere.",
      source: "web:http://127.0.0.1:8420/customers/globex",
    }),
  ];

  it("filters by kind and by content or source", () => {
    expect(filterMemories(memories, "", null).map((m) => m.id)).toEqual(["1", "2"]);
    expect(filterMemories(memories, "", "observation").map((m) => m.id)).toEqual(["2"]);
    expect(filterMemories(memories, "APPROVAL", null).map((m) => m.id)).toEqual(["1"]);
    expect(filterMemories(memories, "globex", null).map((m) => m.id)).toEqual(["2"]);
    expect(filterMemories(memories, "8420", "preference")).toEqual([]);
  });
});

describe("confidenceLabel", () => {
  it("prints at most two places", () => {
    expect(confidenceLabel(1)).toBe("confidence 1");
    expect(confidenceLabel(0.4)).toBe("confidence 0.4");
    expect(confidenceLabel(0.123)).toBe("confidence 0.12");
  });

  it("clamps what the runtime could not have stored", () => {
    expect(confidenceLabel(1.5)).toBe("confidence 1");
    expect(confidenceLabel(-0.2)).toBe("confidence 0");
    expect(confidenceLabel(Number.NaN)).toBe("confidence unknown");
  });
});

describe("memoryRevision", () => {
  const stored = memory("1", { content: "old", confidence: 0.5 });

  it("sends nothing when nothing changed", () => {
    expect(memoryRevision(stored, "old", 0.5)).toBeNull();
  });

  it("refuses empty content rather than forgetting by another name", () => {
    expect(memoryRevision(stored, "   ", 0.9)).toBeNull();
  });

  it("sends the confidence only when it changed", () => {
    expect(memoryRevision(stored, "new", 0.5)).toEqual({ content: "new", confidence: undefined });
    expect(memoryRevision(stored, "old", 0.75)).toEqual({ content: "old", confidence: 0.75 });
  });
});
