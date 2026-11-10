import { describe, expect, it, vi } from "vitest";

import type { AuditRecordView } from "../bindings/AuditRecordView";
import type { CadenceInput } from "../bindings/CadenceInput";
import type { EventView } from "../bindings/EventView";
import type { IntegrationView } from "../bindings/IntegrationView";
import type { MemoryView } from "../bindings/MemoryView";
import type { NetworkCredentialView } from "../bindings/NetworkCredentialView";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { SettingsView } from "../bindings/SettingsView";
import type { StartedTask } from "../bindings/StartedTask";
import type { ToolUsageView } from "../bindings/ToolUsageView";
import { fixtureInvoke, fixtureSubscribe } from "./fixtures";

/**
 * A copy of the fixtures as they are on load. A removal changes the module's
 * state, so each test that removes something takes its own copy and leaves the
 * shared one, and every other test, as they were.
 */
async function freshFixtures(): Promise<typeof fixtureInvoke> {
  vi.resetModules();
  return (await import("./fixtures")).fixtureInvoke;
}

const sum = (counts: readonly number[]) => counts.reduce((total, count) => total + count, 0);

const usageOver = (days?: number) =>
  fixtureInvoke<ToolUsageView[]>("tool_usage", days === undefined ? {} : { days });

/**
 * Fixtures stand in for the runtime while screens are built, so they refuse
 * what the runtime refuses. A screen that has only ever met a fixture that
 * says yes has never shown its refusal.
 */
describe("fixtures", () => {
  it("retry only a task whose latest attempt failed or was cancelled", async () => {
    // task-0003's only attempt was abandoned, which is a failure.
    const started = await fixtureInvoke<StartedTask>("retry_task", { taskId: "task-0003" });
    expect(started.task_id).toBe("task-0003");

    // task-0002 failed once and then succeeded: retrying repeats its effects.
    await expect(fixtureInvoke("retry_task", { taskId: "task-0002" })).rejects.toMatch(/succeeded/);
    // task-0001 is still going.
    await expect(fixtureInvoke("retry_task", { taskId: "task-0001" })).rejects.toMatch(/running/);
    await expect(fixtureInvoke("retry_task", { taskId: "task-none" })).rejects.toMatch(/never been run/);
  });

  it("open an audit record only by an identity it has", async () => {
    const events = await fixtureInvoke<EventView[]>("activity", {});
    const first = events[0]!;
    const record = await fixtureInvoke<AuditRecordView>("audit_record", { eventId: first.id });
    expect(record.id).toBe(first.id);

    await expect(fixtureInvoke("audit_record", { eventId: "no-such-record" })).rejects.toMatch(
      /no audit record/,
    );
  });
  it("refuse a scheduler pace the runtime refuses, and accept one it allows", async () => {
    await expect(
      fixtureInvoke("set_scheduler_running", {
        enabled: true,
        tickSeconds: 4,
        maxConcurrentRuns: 1,
      }),
    ).rejects.toMatch(/5 seconds/);
    await expect(
      fixtureInvoke("set_scheduler_running", {
        enabled: true,
        tickSeconds: 30,
        maxConcurrentRuns: 0,
      }),
    ).rejects.toMatch(/at least one run/);
    const started = await fixtureInvoke<SchedulerView>("set_scheduler_running", {
      enabled: true,
      tickSeconds: 5,
      maxConcurrentRuns: 1,
    });
    expect(started.running).toBe(true);
  });

  it("refuse a memory kind the runtime does not have, and record a note as the operator's", async () => {
    await expect(
      fixtureInvoke("list_memories", { agentId: "agent-sales", kind: "rumour" }),
    ).rejects.toMatch(/not a kind of memory/);
    const remembered = await fixtureInvoke<MemoryView>("remember", {
      input: {
        agent_id: "agent-sales",
        kind: "fact",
        content: "x",
        confidence: null,
        source: "web",
      },
    });
    expect(remembered.source).toBe("user");
    expect(remembered.source_untrusted).toBe(false);
  });

  it("refuse a cadence that names two kinds, and a one-off with no time", async () => {
    const twoKinds: CadenceInput = {
      kind: "every",
      seconds: 3600,
      expression: "0 9 * * *",
      clock: null,
    };
    await expect(fixtureInvoke("check_cadence", { cadence: twoKinds })).rejects.toMatch(
      /exactly one/,
    );

    const once: CadenceInput = { kind: "once", seconds: null, expression: null, clock: null };
    await expect(
      fixtureInvoke("create_schedule", {
        input: {
          agent_id: "agent-sales",
          name: "n",
          objective: "o",
          cadence: once,
          first_run_at: null,
        },
      }),
    ).rejects.toMatch(/needs a time/);
  });

  it("count tool usage a day at a time, every day of the window, summing to the calls", async () => {
    for (const days of [1, 7, 30, 90]) {
      const usage = await usageOver(days);
      expect(usage.length).toBeGreaterThan(0);
      for (const row of usage) {
        expect(row.daily, `${row.tool} over ${days} days`).toHaveLength(days);
        expect(sum(row.daily), `${row.tool} over ${days} days`).toBe(row.calls);
        // No outcome claims more calls than the window has.
        expect(row.executed + row.denied + row.approval_denied).toBeLessThanOrEqual(row.calls);
        expect(row.failed).toBeLessThanOrEqual(row.executed);
        expect(row.under_taint).toBeLessThanOrEqual(row.calls);
        expect(row.approvals).toBeLessThanOrEqual(row.calls);
        expect(row.last_used_at === null).toBe(row.calls === 0);
      }
      const calls = usage.map((row) => row.calls);
      expect(calls, "busiest first").toEqual([...calls].sort((a, b) => b - a));
    }
  });

  it("answer the week by default, and today and the month in agreement with it", async () => {
    const week = await usageOver();
    expect(week.every((row) => row.daily.length === 7)).toBe(true);
    // The week is the table the other screens were built against.
    expect(week.find((row) => row.tool === "browser.navigate")).toMatchObject({
      calls: 59,
      executed: 58,
      approval_denied: 1,
      failed: 2,
    });

    const today = await usageOver(1);
    const month = await usageOver(30);
    for (const row of week) {
      expect(today.find((each) => each.tool === row.tool)?.calls).toBe(row.daily.at(-1));
      const monthly = month.find((each) => each.tool === row.tool);
      expect(monthly?.daily.slice(-7)).toEqual(row.daily);
      expect(monthly?.calls).toBeGreaterThanOrEqual(row.calls);
    }

    // Clamped as the runtime clamps: there is no window shorter than today.
    expect((await usageOver(0)).every((row) => row.daily.length === 1)).toBe(true);
  });

  it("give a tool last used before midnight no calls today, without changing its week", async () => {
    const early = new Date();
    early.setHours(0, 30, 0, 0);
    // Only the clock: the fixtures still answer after a real delay.
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(early);
    try {
      const invoke = await freshFixtures();
      const named = (usage: ToolUsageView[], tool: string) =>
        usage.find((row) => row.tool === tool);
      const today = await invoke<ToolUsageView[]>("tool_usage", { days: 1 });
      const week = await invoke<ToolUsageView[]>("tool_usage", { days: 7 });
      // Last written to nearly three hours ago, which at half past midnight
      // was yesterday evening.
      expect(named(today, "filesystem.write")).toMatchObject({ calls: 0, last_used_at: null });
      expect(named(week, "filesystem.write")?.calls).toBe(6);
      // Typed into a minute ago, which is still today.
      expect(named(today, "browser.type")?.calls).toBeGreaterThan(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("remove a credential by the address it was stored under, and leave its account tokenless", async () => {
    const invoke = await freshFixtures();
    const before = await invoke<NetworkCredentialView[]>("list_network_credentials");
    // Read as storing reads it: case and the default port do not matter.
    await invoke("remove_network_credential", { origin: "https://API.github.com:443", name: "work" });

    const after = await invoke<NetworkCredentialView[]>("list_network_credentials");
    expect(after, "a new list, so a screen that reads it again redraws").not.toBe(before);
    expect(after.map((stored) => `${stored.origin} ${stored.name}`)).toEqual([
      "http://127.0.0.1:8420 default",
      "http://127.0.0.1:8420 reports",
    ]);
    const [github] = await invoke<IntegrationView[]>("list_integrations");
    const work = github?.accounts.find((account) => account.id === "acct-work");
    expect(work?.credential_present).toBe(false);

    // Removing what is not there is not refused; an address that is not one is.
    await expect(
      invoke("remove_network_credential", { origin: "https://api.github.com", name: "work" }),
    ).resolves.toBeNull();
    await expect(
      invoke("remove_network_credential", { origin: "ftp://files.example", name: "work" }),
    ).rejects.toMatch(/not an http or https URL/);

    for (const name of ["default", "reports"]) {
      await invoke("remove_network_credential", { origin: "http://127.0.0.1:8420", name });
    }
    expect(await invoke("list_network_credentials")).toEqual([]);
  });

  it("unbind an account and its token, down to an integration with nothing bound", async () => {
    const invoke = await freshFixtures();
    await invoke("unbind_integration", { accountId: "acct-work" });

    let [github] = await invoke<IntegrationView[]>("list_integrations");
    expect(github?.accounts.map((account) => account.id)).toEqual(["acct-ghe"]);
    const credentials = await invoke<NetworkCredentialView[]>("list_network_credentials");
    expect(credentials.map((stored) => stored.origin)).not.toContain("https://api.github.com");
    expect(credentials, "the CRM's credentials were nobody's token").toHaveLength(2);

    await expect(invoke("unbind_integration", { accountId: "acct-work" })).rejects.toMatch(
      /no integration account/,
    );
    await invoke("unbind_integration", { accountId: "acct-ghe" });
    [github] = await invoke<IntegrationView[]>("list_integrations");
    // Still listed, with its tools: what binding would offer is worth seeing.
    expect(github?.accounts).toEqual([]);
    expect(github?.tools.length).toBeGreaterThan(0);
  });

  it("remove a provider key, and refuse a provider the runtime cannot build", async () => {
    const invoke = await freshFixtures();
    await invoke("remove_provider_key", { provider: "anthropic" });

    const settings = await invoke<SettingsView>("settings");
    expect(settings.providers.find((provider) => provider.id === "anthropic")).toMatchObject({
      configured: false,
      hint: null,
      source: null,
    });
    expect(settings.providers.some((provider) => provider.configured)).toBe(false);
    await expect(invoke("remove_provider_key", { provider: "gemini" })).rejects.toMatch(
      /unknown provider/,
    );

    // The shared fixtures were not touched.
    const shared = await fixtureInvoke<SettingsView>("settings");
    expect(shared.providers.find((provider) => provider.id === "anthropic")?.configured).toBe(true);
  });

  it("replay live events without a place in the chain, as the real stream sends them", () => {
    vi.useFakeTimers();
    try {
      const received: EventView[] = [];
      const stop = fixtureSubscribe<EventView>("agentos://activity", (event) =>
        received.push(event),
      );
      vi.advanceTimersByTime(6_000);
      stop();
      expect(received.length).toBeGreaterThan(0);
      expect(received.every((event) => event.sequence === null)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
