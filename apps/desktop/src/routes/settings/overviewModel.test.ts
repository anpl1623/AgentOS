import { describe, expect, it } from "vitest";

import type { IntegrationView } from "../../bindings/IntegrationView";
import type { SettingsView } from "../../bindings/SettingsView";
import { auditFindings } from "./auditModel";
import { type OverviewInput, type OverviewTile, overviewTiles } from "./overviewModel";
import { SECTIONS } from "./sectionsModel";
import { NOW, account, minutesFrom, scheduler } from "./testData";

function settings(extra: Partial<SettingsView> = {}): SettingsView {
  return {
    data_dir: "/d",
    workspace: "/d/w",
    database: "/d/db",
    keychain_available: true,
    keychain_reason: null,
    providers: [
      { id: "anthropic", configured: true, hint: "sk…", source: "system keychain", note: "" },
      { id: "openai", configured: false, hint: null, source: null, note: "" },
    ],
    browser_path: null,
    browser_hint: null,
    tools: [],
    ...extra,
  };
}

function github(accounts: IntegrationView["accounts"]): IntegrationView {
  return {
    id: "github",
    display_name: "GitHub",
    default_host: "https://api.github.com",
    tools: [],
    accounts,
  };
}

const NOTHING: OverviewInput = {
  settings: null,
  integrations: null,
  scheduler: null,
  audit: null,
  checking: false,
  checkError: null,
  now: NOW,
};

function tile(input: Partial<OverviewInput>, label: string): OverviewTile {
  const found = overviewTiles({ ...NOTHING, ...input }).find((each) => each.label === label);
  if (found === undefined) throw new Error(`no tile labelled ${label}`);
  return found;
}

describe("overviewTiles", () => {
  it("leads each tile to a section on the screen", () => {
    const ids = new Set(SECTIONS.map((entry) => entry.id));
    for (const each of overviewTiles(NOTHING)) expect(ids.has(each.section)).toBe(true);
  });

  it("shows nothing as a figure until it has been read, and stays quiet meanwhile", () => {
    for (const label of ["Provider keys", "Accounts", "Scheduler"]) {
      const each = tile({}, label);
      expect(each.value).toBeNull();
      expect(each.tone).toBe("accent");
    }
  });

  it("counts the providers with a key and names them", () => {
    const keys = tile({ settings: settings() }, "Provider keys");
    expect(keys.value).toBe(1);
    expect(keys.hint).toBe("of 2 providers: anthropic");
    expect(keys.tone).toBe("accent");
  });

  it("asks for attention when there is no keychain to keep keys in", () => {
    const keys = tile({ settings: settings({ keychain_available: false }) }, "Provider keys");
    expect(keys.tone).toBe("warn");
    expect(keys.hint).toMatch(/environment/);
  });

  it("draws an account with no token as a failure, and says why", () => {
    const broken = tile(
      { integrations: [github([account("work"), account("ghe", { credential_present: false })])] },
      "Accounts",
    );
    // The loud figure counts what is wrong, not every account.
    expect(broken.value).toBe("1 broken");
    expect(broken.tone).toBe("danger");
    expect(broken.hint).toBe("of 2 bound: no token, so its calls fail");
    const both = tile(
      {
        integrations: [
          github([
            account("a", { credential_present: false }),
            account("b", { credential_present: false }),
          ]),
        ],
      },
      "Accounts",
    );
    expect(both).toMatchObject({
      value: "2 broken",
      hint: "of 2 bound: no token, so their calls fail",
    });
    const healthy = tile({ integrations: [github([account("work")])] }, "Accounts");
    expect(healthy).toMatchObject({ value: 1, tone: "accent", hint: "on GitHub" });
    expect(tile({ integrations: [github([])] }, "Accounts").hint).toBe("None bound yet");
  });

  it("asks for attention for work the scheduler can never start or has let slip", () => {
    const stuck = tile({ scheduler: scheduler({ unreachable_tasks: 1 }) }, "Scheduler");
    expect(stuck).toMatchObject({ value: "Off", tone: "warn", hint: "1 task can never start" });
    const late = tile({ scheduler: scheduler({ overdue: 2, error: "the lease was lost" }) }, "Scheduler");
    expect(late.hint).toBe("Stopped on its own · 2 schedules overdue");
  });

  it("says a quiet scheduler is off or when it next fires", () => {
    expect(tile({ scheduler: scheduler() }, "Scheduler")).toMatchObject({
      value: "Off",
      tone: "accent",
      hint: "2 schedules waiting",
    });
    expect(tile({ scheduler: scheduler({ running: true }) }, "Scheduler")).toMatchObject({
      value: "Running",
      tone: "ok",
      hint: "Next firing in 30m",
    });
  });

  it("calls an unchecked chain unverified, and colours it only once checked", () => {
    expect(tile({}, "Audit chain")).toMatchObject({ value: "Unverified", tone: "accent" });
    const gaps = auditFindings(minutesFrom(-2), { breaks: [], gaps: ["g"] });
    expect(tile({ audit: gaps }, "Audit chain")).toMatchObject({
      value: "1 gap",
      tone: "warn",
      hint: "Checked 2m ago",
    });
    const clean = auditFindings(minutesFrom(0), { breaks: [], gaps: [] });
    expect(tile({ audit: clean }, "Audit chain")).toMatchObject({
      value: "Intact",
      tone: "ok",
      hint: "Checked just now",
    });
  });

  it("never says a chain with a break was verified", () => {
    const broken = auditFindings(minutesFrom(-5), { breaks: ["b"], gaps: [] });
    const each = tile({ audit: broken }, "Audit chain");
    expect(each).toMatchObject({ value: "1 break", tone: "danger", hint: "Checked 5m ago" });
    expect(each.hint).not.toMatch(/verified/i);
  });

  it("says a check is running, keeping when the last one finished", () => {
    expect(tile({ checking: true }, "Audit chain")).toMatchObject({
      value: "Checking…",
      tone: "info",
      hint: "Recomputing every hash",
    });
    const clean = auditFindings(minutesFrom(-3), { breaks: [], gaps: [] });
    expect(tile({ checking: true, audit: clean }, "Audit chain").hint).toBe(
      "Recomputing every hash; last checked 3m ago",
    );
  });

  it("says where the header's check landed when it could not run", () => {
    expect(tile({ checkError: "the log is locked" }, "Audit chain")).toMatchObject({
      value: "Check failed",
      tone: "danger",
      hint: "The check could not run",
    });
    const clean = auditFindings(minutesFrom(-3), { breaks: [], gaps: [] });
    expect(tile({ checkError: "x", audit: clean }, "Audit chain")).toMatchObject({
      value: "Check failed",
      hint: "The check could not run; last checked 3m ago",
    });
    // A check that is running again outranks the failure before it.
    expect(tile({ checkError: "x", checking: true }, "Audit chain").value).toBe("Checking…");
  });
});
