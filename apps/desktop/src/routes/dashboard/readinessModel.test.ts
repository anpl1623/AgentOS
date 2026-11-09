import { describe, expect, it } from "vitest";

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { SettingsView } from "../../bindings/SettingsView";
import { type ReadinessItem, SETUP_STEPS, readiness, setupProgress } from "./readinessModel";

const NOW = Date.parse("2026-10-06T12:00:00Z");
const minutesAgo = (n: number) => new Date(NOW - n * 60_000).toISOString();

function agent(name: string, provider: string, extra: Partial<AgentSummary> = {}): AgentSummary {
  return {
    id: `agent-${name}`,
    name,
    provider,
    model: "model",
    status: "enabled",
    tools: [],
    max_steps: 24,
    created_at: minutesAgo(1000),
    ...extra,
  };
}

function settings(extra: Partial<SettingsView> = {}): SettingsView {
  return {
    data_dir: "/data",
    workspace: "/data/workspace",
    database: "/data/agentos.db",
    keychain_available: true,
    keychain_reason: null,
    providers: [
      { id: "anthropic", configured: true, hint: "sk-…", source: "system keychain", note: "" },
      { id: "openai", configured: false, hint: null, source: null, note: "" },
      { id: "ollama", configured: false, hint: null, source: null, note: "" },
      { id: "mock", configured: false, hint: null, source: null, note: "" },
    ],
    browser_path: "/usr/bin/chromium",
    browser_hint: null,
    tools: [
      {
        name: "browser.navigate",
        domain: "browser",
        description: "",
        risk: "medium",
        returns_untrusted_data: true,
        capabilities: ["browser.navigate"],
      },
      {
        name: "filesystem.read",
        domain: "filesystem",
        description: "",
        risk: "low",
        returns_untrusted_data: true,
        capabilities: ["filesystem.read"],
      },
    ],
    ...extra,
  };
}

const unkeyed = (providers: SettingsView["providers"]) =>
  providers.map((provider) => ({ ...provider, configured: false, hint: null, source: null }));

describe("readiness", () => {
  it("says nothing until both settings and agents have loaded", () => {
    expect(readiness(null, [])).toEqual([]);
    expect(readiness(settings({ providers: [] }), null)).toEqual([]);
  });

  it("is empty for an installation with a keyed agent and a browser", () => {
    expect(readiness(settings(), [agent("sales", "anthropic")])).toEqual([]);
  });

  it("asks for an agent first thing on a fresh install, pointing at Agents", () => {
    const items = readiness(settings(), []);
    expect(items.map((item) => item.id)).toEqual(["agent"]);
    expect(items[0]?.target).toEqual({ name: "agents" });
  });

  it("says when every agent is disabled", () => {
    const items = readiness(settings(), [agent("sales", "anthropic", { status: "disabled" })]);
    expect(items.map((item) => item.id)).toEqual(["agent"]);
    expect(items[0]?.problem).toMatch(/disabled/);
  });

  it("asks for a key when there is none and no agent to say which, pointing at Settings", () => {
    const items = readiness(settings({ providers: unkeyed(settings().providers) }), []);
    expect(items.map((item) => item.id)).toEqual(["provider", "agent"]);
    expect(items[0]?.target).toEqual({ name: "settings" });
  });

  it("names the agent whose provider has no key", () => {
    const items = readiness(settings(), [agent("writer", "openai")]);
    expect(items.map((item) => item.id)).toEqual(["provider"]);
    expect(items[0]?.problem).toBe("writer uses openai, which has no key.");
  });

  it("needs no key for an agent on a local or built-in provider", () => {
    const providers = unkeyed(settings().providers);
    expect(readiness(settings({ providers }), [agent("ops", "ollama")])).toEqual([]);
    expect(readiness(settings({ providers }), [agent("test", "mock")])).toEqual([]);
  });

  it("does not hold a disabled agent's missing key against the installation", () => {
    const items = readiness(settings(), [
      agent("sales", "anthropic"),
      agent("old", "openai", { status: "disabled" }),
    ]);
    expect(items).toEqual([]);
  });

  it("does not guess about a provider this build does not list", () => {
    expect(readiness(settings(), [agent("x", "someday")])).toEqual([]);
  });

  it("raises a missing keychain only when a key is needed and cannot be saved", () => {
    const withoutKeychain = { keychain_available: false, keychain_reason: "no secret service" };
    // Configured from the environment: nothing is stopped, nothing is raised.
    expect(readiness(settings(withoutKeychain), [agent("sales", "anthropic")])).toEqual([]);

    const items = readiness(
      settings({ ...withoutKeychain, providers: unkeyed(settings().providers) }),
      [agent("sales", "anthropic")],
    );
    expect(items.map((item) => item.id)).toEqual(["provider", "keychain"]);
    expect(items[1]?.problem).toContain("no secret service");
  });

  it("raises a missing browser for an enabled agent holding a browser tool", () => {
    const items = readiness(settings({ browser_path: null, browser_hint: "Install Chrome." }), [
      agent("sales", "anthropic", { tools: ["browser.navigate"] }),
    ]);
    expect(items.map((item) => item.id)).toEqual(["browser"]);
    expect(items[0]?.problem).toContain("sales has browser tools");
    expect(items[0]?.remedy).toBe("Install Chrome.");
  });

  it("knows a browser tool by its catalogue domain, not by its name", () => {
    const base = settings();
    const renamed = settings({
      browser_path: null,
      tools: base.tools.map((tool) =>
        tool.name === "browser.navigate" ? { ...tool, name: "web.open" } : tool,
      ),
    });
    const items = readiness(renamed, [agent("sales", "anthropic", { tools: ["web.open"] })]);
    expect(items.map((item) => item.id)).toEqual(["browser"]);
  });

  it("leaves a missing browser alone when no enabled agent could use one", () => {
    expect(
      readiness(settings({ browser_path: null }), [
        agent("ops", "anthropic", { tools: ["filesystem.read"] }),
        agent("old", "anthropic", { status: "disabled", tools: ["browser.navigate"] }),
      ]),
    ).toEqual([]);
  });
});

describe("setupProgress", () => {
  const item = (id: ReadinessItem["id"]): ReadinessItem => ({
    id,
    problem: `${id} missing`,
    remedy: `fix ${id}`,
    target: { name: "settings" },
  });

  it("counts every check readiness did not raise as done", () => {
    const progress = setupProgress([item("agent")]);
    expect(progress.total).toBe(SETUP_STEPS.length);
    expect(progress.done).toBe(SETUP_STEPS.length - 1);
    expect(progress.percent).toBe(75);
  });

  it("leads with what is missing, then the steps already done in their own order", () => {
    const progress = setupProgress([item("browser"), item("provider")]);
    expect(progress.steps.map((step) => step.id)).toEqual([
      "provider",
      "browser",
      "keychain",
      "agent",
    ]);
    expect(progress.steps[0]?.missing?.remedy).toBe("fix provider");
    expect(progress.steps[2]?.missing).toBeNull();
  });

  it("is complete when nothing is missing", () => {
    const progress = setupProgress([]);
    expect(progress.done).toBe(progress.total);
    expect(progress.percent).toBe(100);
    expect(progress.steps.every((step) => step.missing === null)).toBe(true);
  });

  it("ticks the keychain and the browser only in words true when nothing needed them", () => {
    // No keychain and no browser on this machine, and nothing that needs
    // either: readiness raises neither, so both are ticked, and the words
    // beside the ticks must not claim that a keychain or a browser exists.
    const machine = settings({ keychain_available: false, browser_path: null });
    const missing = readiness(machine, [agent("ops", "anthropic", { tools: ["filesystem.read"] })]);
    expect(missing).toEqual([]);
    const done = new Map(setupProgress(missing).steps.map((step) => [step.id, step.done]));
    expect(done.get("keychain")).toBe("Keys can be kept, or none is needed");
    expect(done.get("browser")).toBe("No agent is missing a browser");
  });

  it("agrees with readiness on a fresh install", () => {
    const fresh = readiness(settings({ providers: unkeyed(settings().providers) }), []);
    const progress = setupProgress(fresh);
    expect(progress.done).toBe(progress.total - fresh.length);
    expect(progress.steps.slice(0, fresh.length).every((step) => step.missing !== null)).toBe(true);
  });
});
