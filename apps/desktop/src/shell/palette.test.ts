import { describe, expect, it } from "vitest";

import type { AgentSummary } from "../bindings/AgentSummary";
import {
  PER_GROUP,
  type PaletteEntry,
  agentEntries,
  matchIndex,
  moveActive,
  rankEntries,
  screenEntries,
} from "./palette";

function agent(name: string, status = "enabled"): AgentSummary {
  return {
    id: `id-${name}`,
    name,
    provider: "anthropic",
    model: "claude-opus-5",
    status,
    tools: [],
    max_steps: 20,
    created_at: "2026-10-01T00:00:00Z",
  };
}

describe("matchIndex", () => {
  it("matches a case-insensitive subsequence at its first character", () => {
    expect(matchIndex("Approvals", "apv")).toBe(0);
    expect(matchIndex("crm-assistant", "ASSIST")).toBe(4);
    expect(matchIndex("Settings", "tgs")).toBe(2);
  });

  it("ignores whitespace in the query", () => {
    expect(matchIndex("crm-assistant", "crm assist")).toBe(0);
  });

  it("reports no match when the characters are out of order", () => {
    expect(matchIndex("Agents", "sa")).toBe(-1);
    expect(matchIndex("Tasks", "z")).toBe(-1);
  });

  it("matches everything for an empty query", () => {
    expect(matchIndex("anything", "")).toBe(0);
    expect(matchIndex("anything", "   ")).toBe(0);
  });
});

describe("rankEntries", () => {
  const entries = [...screenEntries(), ...agentEntries([agent("triage"), agent("tasker", "disabled")])];

  it("lists screens before agents, each in its own order, for an empty query", () => {
    const ranked = rankEntries(entries, "");
    expect(ranked.map((entry) => entry.group)).toEqual([
      ...Array(7).fill("Screens"),
      "Agents",
      "Agents",
    ]);
    expect(ranked[0]?.label).toBe("Dashboard");
    expect(ranked.slice(-2).map((entry) => entry.label)).toEqual(["triage", "tasker"]);
  });

  it("ranks by how closely the label matches, then where, then by list order", () => {
    const entry = (group: PaletteEntry["group"], label: string): PaletteEntry => ({
      id: `${group}:${label}`,
      group,
      label,
      hint: "",
      badge: null,
      route: { name: "dashboard" },
    });
    const ranked = rankEntries(
      [
        entry("Agents", "a-tasker"),
        entry("Screens", "xx-task"),
        entry("Agents", "tasks"),
        entry("Screens", "task"),
        entry("Screens", "x-task"),
        entry("Agents", "b-tasker"),
        entry("Screens", "nothing"),
      ],
      "tas",
    );
    expect(ranked.map((each) => each.label)).toEqual([
      "task",
      "x-task",
      "xx-task",
      "tasks",
      "a-tasker",
      "b-tasker",
    ]);
  });

  it("puts an agent named exactly by the query ahead of a screen whose description it threads", () => {
    // `ops` is in "Agents, their tools and policies" letter by letter, and it
    // is the name of an agent. Enter must open the agent.
    const withOps = [...screenEntries(), ...agentEntries([agent("sales"), agent("ops")])];
    const ranked = rankEntries(withOps, "ops");
    expect(ranked[0]).toMatchObject({ group: "Agents", label: "ops" });
    expect(ranked[0]?.route).toEqual({ name: "agents", agent: "ops" });
    expect(ranked.some((entry) => entry.group === "Screens" && entry.label === "Agents")).toBe(true);
  });

  it("keeps screens first when both groups match equally well", () => {
    const ranked = rankEntries(
      [...screenEntries(), ...agentEntries([agent("settings-bot")])],
      "sett",
    );
    expect(ranked[0]?.label).toBe("Settings");
    expect(ranked.at(-1)?.label).toBe("settings-bot");
  });

  it("searches the hint as well as the label", () => {
    const ranked = rankEntries(entries, "policies");
    expect(ranked.map((entry) => entry.label)).toContain("Agents");
  });

  it("keeps at most eight from each group", () => {
    const many = agentEntries(Array.from({ length: 20 }, (_, i) => agent(`agent-${i}`)));
    const ranked = rankEntries([...screenEntries(), ...many], "");
    expect(ranked.filter((entry) => entry.group === "Agents")).toHaveLength(PER_GROUP);
  });

  it("offers only places to go", () => {
    // An entry carries a route and nothing callable, so choosing one can only navigate.
    for (const entry of entries) {
      for (const value of Object.values(entry as unknown as Record<string, unknown>)) {
        expect(typeof value).not.toBe("function");
      }
      expect(entry.route.name).toBeTypeOf("string");
    }
  });
});

describe("agentEntries", () => {
  it("lists a disabled agent with a badge rather than leaving it out", () => {
    const [on, off] = agentEntries([agent("a"), agent("b", "disabled")]) as [
      PaletteEntry,
      PaletteEntry,
    ];
    expect(on.badge).toBeNull();
    expect(off.badge).toBe("disabled");
    expect(off.route).toEqual({ name: "agents", agent: "b" });
  });
});

describe("moveActive", () => {
  it("wraps at both ends", () => {
    expect(moveActive(0, -1, 3)).toBe(2);
    expect(moveActive(2, 1, 3)).toBe(0);
    expect(moveActive(1, 1, 3)).toBe(2);
    expect(moveActive(0, 1, 0)).toBe(0);
  });
});
