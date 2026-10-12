import { describe, expect, it } from "vitest";

import type { Route, RouteName } from "../routes/route";
import { fromHash, toHash } from "./hash";

/**
 * Every shape of every route. Keyed by name, so a screen added to the union
 * without samples here fails to typecheck rather than going untested.
 */
const SAMPLES: Record<RouteName, Route[]> = {
  dashboard: [{ name: "dashboard" }],
  approvals: [{ name: "approvals" }, { name: "approvals", focus: "approval-0001" }],
  tasks: [{ name: "tasks" }, { name: "tasks", runId: "run-0001" }],
  agents: [{ name: "agents" }, { name: "agents", agent: "crm-assistant" }],
  activity: [{ name: "activity" }, { name: "activity", runId: "run-0001" }],
  schedules: [{ name: "schedules" }],
  settings: [{ name: "settings" }],
};

/** Names that would break a codec that split, decoded or escaped carelessly. */
const AWKWARD = [
  "ops/billing",
  "a#b",
  "100% sure",
  "%2F",
  "%",
  "two words",
  "run",
  "agents",
  "émile",
  "名前",
  "🦀 crab",
  "../settings",
  "?q=1&x",
  "trailing/",
];

describe("toHash and fromHash", () => {
  it("round-trip every route", () => {
    for (const route of Object.values(SAMPLES).flat()) {
      expect(fromHash(toHash(route))).toEqual(route);
    }
  });

  it("give each route its own address", () => {
    const hashes = Object.values(SAMPLES).flat().map(toHash);
    expect(new Set(hashes).size).toBe(hashes.length);
  });

  it("round-trip identifiers containing separators, escapes and non-ASCII", () => {
    for (const value of AWKWARD) {
      const routes: Route[] = [
        { name: "agents", agent: value },
        { name: "approvals", focus: value },
        { name: "tasks", runId: value },
        { name: "activity", runId: value },
      ];
      for (const route of routes) {
        expect(fromHash(toHash(route))).toEqual(route);
      }
    }
  });

  it("write the addresses links are built from", () => {
    expect(toHash({ name: "approvals", focus: "a1" })).toBe("#/approvals/a1");
    expect(toHash({ name: "tasks", runId: "r1" })).toBe("#/tasks/run/r1");
    expect(toHash({ name: "agents", agent: "ops/billing" })).toBe("#/agents/ops%2Fbilling");
  });

  it("read a hash with or without its leading #", () => {
    expect(fromHash("/agents/x")).toEqual({ name: "agents", agent: "x" });
    expect(fromHash("#/agents/x")).toEqual({ name: "agents", agent: "x" });
  });

  it("treat an empty identifier as none", () => {
    expect(toHash({ name: "agents", agent: "" })).toBe("#/agents");
    expect(toHash({ name: "tasks", runId: "" })).toBe("#/tasks");
  });
});

describe("fromHash", () => {
  const dashboard = { name: "dashboard" };

  it("gives the dashboard for an empty hash", () => {
    for (const hash of ["", "#", "#/", "/"]) {
      expect(fromHash(hash)).toEqual(dashboard);
    }
  });

  it("gives the dashboard for unknown and malformed hashes rather than throwing", () => {
    const malformed = [
      "#/nowhere",
      "#/Agents",
      "#agents",
      "#//agents",
      "#/agents/",
      "#/agents/a/b",
      "#/approvals/",
      "#/tasks/run",
      "#/tasks/run/",
      "#/tasks/r1",
      "#/tasks/run/r1/extra",
      "#/activity/trace/r1",
      "#/settings/extra",
      "#/dashboard/x",
      "#/agents/%",
      "#/agents/%E0%A4%A",
      "#/agents/%ZZ",
      "#/__proto__",
      "#/constructor",
      "#/toString",
    ];
    for (const hash of malformed) {
      expect(() => fromHash(hash)).not.toThrow();
      expect(fromHash(hash)).toEqual(dashboard);
    }
  });

  it("decodes an identifier the address bar left unescaped", () => {
    expect(fromHash("#/agents/émile")).toEqual({ name: "agents", agent: "émile" });
  });
});
