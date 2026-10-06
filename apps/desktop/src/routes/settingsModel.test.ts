import { describe, expect, it } from "vitest";

import type { AgentSummary } from "../bindings/AgentSummary";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { ToolView } from "../bindings/ToolView";
import {
  asSentence,
  catalogue,
  countEntries,
  credentialReady,
  credentialRows,
  entryMatches,
  filterTerms,
  grantLine,
  holdersOf,
  parseCount,
  readPacing,
  relative,
  schedulerFacts,
  schedulerNotices,
  schedulerSummary,
  verificationAnnouncement,
  verificationLine,
} from "./settingsModel";

const NOW = Date.parse("2026-10-06T12:00:00Z");
const minutesFrom = (n: number) => new Date(NOW + n * 60_000).toISOString();

function tool(name: string, risk: string, extra: Partial<ToolView> = {}): ToolView {
  return {
    name,
    domain: name.split(".")[0] ?? name,
    description: `The ${name} tool.`,
    risk,
    returns_untrusted_data: false,
    capabilities: [name],
    ...extra,
  };
}

function agent(name: string, tools: string[], status = "enabled"): AgentSummary {
  return {
    id: `id-${name}`,
    name,
    provider: "mock",
    model: "mock",
    status,
    tools,
    max_steps: 10,
    created_at: minutesFrom(-1000),
  };
}

function scheduler(extra: Partial<SchedulerView> = {}): SchedulerView {
  return {
    running: false,
    tick_seconds: 30,
    max_concurrent_runs: 1,
    started_at: null,
    error: null,
    active_schedules: 2,
    next_fire_at: minutesFrom(30),
    overdue: 0,
    runnable_tasks: 1,
    unreachable_tasks: 0,
    ...extra,
  };
}

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

describe("relative", () => {
  it("says which side of now a moment falls, in whole units rounded down", () => {
    expect(relative(minutesFrom(11.9), NOW)).toBe("in 11m");
    expect(relative(minutesFrom(-11.9), NOW)).toBe("11m ago");
    expect(relative(minutesFrom(150), NOW)).toBe("in 2h");
    expect(relative(minutesFrom(-3 * 24 * 60), NOW)).toBe("3d ago");
  });

  it("calls anything within a minute now, and says so of a time it cannot read", () => {
    expect(relative(minutesFrom(0.5), NOW)).toBe("now");
    expect(relative(minutesFrom(-0.5), NOW)).toBe("now");
    expect(relative("not a time", NOW)).toBe("at an unreadable time");
  });
});

describe("asSentence", () => {
  it("capitalises and closes a runtime message without changing its words", () => {
    expect(asSentence("a tick shorter than 5 seconds is a busy loop")).toBe(
      "A tick shorter than 5 seconds is a busy loop.",
    );
    expect(asSentence("  already a sentence.  ")).toBe("Already a sentence.");
    expect(asSentence("")).toBe("");
  });
});

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

describe("parseCount and readPacing", () => {
  it("accepts whole numbers only, leaving the range to the runtime", () => {
    expect(parseCount("30")).toBe(30);
    expect(parseCount(" 5 ")).toBe(5);
    // Zero is a whole number; the runtime refuses it in its own words.
    expect(parseCount("0")).toBe(0);
    for (const text of ["", "1.5", "-3", "1e3", "abc", "99999999999999999999"]) {
      expect(parseCount(text)).toBeNull();
    }
  });

  it("reports pacing as unchanged when the fields say what the scheduler does", () => {
    expect(readPacing(scheduler(), "30", "1")).toEqual({
      tick: 30,
      max: 1,
      valid: true,
      changed: false,
    });
  });

  it("reports a change, and an invalid field as one that cannot be sent", () => {
    expect(readPacing(scheduler(), "60", "1").changed).toBe(true);
    const invalid = readPacing(scheduler(), "", "1");
    expect(invalid.valid).toBe(false);
    expect(invalid.changed).toBe(true);
  });
});

describe("schedulerSummary", () => {
  it("says, when off, that schedules are kept and that nothing fires", () => {
    const summary = schedulerSummary(scheduler(), NOW);
    expect(summary).toMatch(/^Off\./);
    expect(summary).toContain("Schedules are kept");
    expect(summary).toContain("nothing fires");
  });

  it("says, when running, since when and how it is paced", () => {
    const summary = schedulerSummary(
      scheduler({ running: true, started_at: minutesFrom(-90), max_concurrent_runs: 3 }),
      NOW,
    );
    expect(summary).toBe(
      "Running, started 1h ago. It looks for due work every 30s, with up to 3 runs at once.",
    );
  });
});

describe("schedulerFacts", () => {
  const value = (facts: ReturnType<typeof schedulerFacts>, label: string) =>
    facts.find((fact) => fact.label === label);

  it("flags overdue schedules and unreachable tasks only when there are some", () => {
    const quiet = schedulerFacts(scheduler(), NOW);
    expect(quiet.every((fact) => !fact.attention)).toBe(true);
    const loud = schedulerFacts(scheduler({ overdue: 2, unreachable_tasks: 1 }), NOW);
    expect(value(loud, "Overdue")).toEqual({ label: "Overdue", value: "2", attention: true });
    expect(value(loud, "Can never start")?.attention).toBe(true);
  });

  it("says when the next schedule fires, and that a passed one is due", () => {
    expect(value(schedulerFacts(scheduler(), NOW), "Next firing")?.value).toBe("in 30m");
    const passed = scheduler({ next_fire_at: minutesFrom(-5) });
    expect(value(schedulerFacts(passed, NOW), "Next firing")?.value).toBe("due 5m ago");
    expect(
      value(schedulerFacts({ ...passed, running: true }, NOW), "Next firing")?.value,
    ).toBe("due now");
    expect(
      value(schedulerFacts(scheduler({ next_fire_at: null }), NOW), "Next firing")?.value,
    ).toBe("nothing scheduled");
  });
});

describe("schedulerNotices", () => {
  const LEASE =
    "the scheduler is already running in another process on this installation, most likely " +
    "`agentos schedule run` in a terminal.";

  it("shows the lease refusal once, though the switch and the status both carry it", () => {
    expect(schedulerNotices(scheduler({ error: LEASE }), LEASE)).toEqual([asSentence(LEASE)]);
  });

  it("shows a stop the scheduler came to on its own, with nobody pressing anything", () => {
    expect(schedulerNotices(scheduler({ error: "the database went away" }), null)).toEqual([
      "The database went away.",
    ]);
  });

  it("keeps a refusal the status does not repeat beside the status's own error", () => {
    expect(
      schedulerNotices(scheduler({ error: LEASE }), "a tick shorter than 5 seconds is a busy loop"),
    ).toEqual(["A tick shorter than 5 seconds is a busy loop.", asSentence(LEASE)]);
  });

  it("drops an old error once the scheduler is running again", () => {
    expect(schedulerNotices(scheduler({ running: true, error: LEASE }), null)).toEqual([]);
    expect(schedulerNotices(null, null)).toEqual([]);
  });
});

describe("verification", () => {
  it("says the chain has not been checked rather than leaving a blank", () => {
    expect(verificationLine(null, NOW)).toBe("not verified since the window opened");
  });

  it("says when it was checked and what was found", () => {
    expect(verificationLine({ at: minutesFrom(-4), problems: [] }, NOW)).toBe(
      "verified 4m ago · intact",
    );
    expect(verificationLine({ at: minutesFrom(-4), problems: ["a", "b"] }, NOW)).toBe(
      "verified 4m ago · 2 problems",
    );
    expect(verificationLine({ at: minutesFrom(0), problems: ["a"] }, NOW)).toBe(
      "verified now · 1 problem",
    );
  });

  it("announces the result as a sentence", () => {
    expect(verificationAnnouncement([])).toBe("Audit chain verified intact.");
    expect(verificationAnnouncement(["a"])).toBe("Audit chain verification found 1 problem.");
    expect(verificationAnnouncement(["a", "b", "c"])).toBe(
      "Audit chain verification found 3 problems.",
    );
  });
});

describe("credentialRows", () => {
  it("orders by origin then name, labelled origin / name", () => {
    const rows = credentialRows([
      { origin: "https://b.example", name: "token" },
      { origin: "https://a.example", name: "zeta" },
      { origin: "https://a.example", name: "alpha" },
    ]);
    expect(rows.map((row) => row.label)).toEqual([
      "https://a.example / alpha",
      "https://a.example / zeta",
      "https://b.example / token",
    ]);
  });

  it("never carries a value, even when the view it is given does", () => {
    // A future view that grew a value, a hint or a mask must still not reach
    // the screen through these rows.
    const leaky = [
      {
        origin: "https://crm.example",
        name: "api",
        value: "s3cr3t",
        hint: "s3…t",
        masked: "••••",
      },
    ];
    const [row] = credentialRows(leaky);
    expect(Object.keys(row!).sort()).toEqual(["key", "label", "name", "origin"]);
    expect(JSON.stringify(row)).not.toContain("s3");
    expect(JSON.stringify(row)).not.toContain("••••");
  });

  it("keys a row unambiguously whatever its parts contain", () => {
    const [first, second] = credentialRows([
      { origin: "https://a / b", name: "c" },
      { origin: "https://a", name: "b / c" },
    ]);
    expect(first!.label).toBe(second!.label);
    expect(first!.key).not.toBe(second!.key);
  });
});

describe("credentialReady", () => {
  it("needs an origin, a name and a secret", () => {
    expect(credentialReady("https://crm.example", "api", "s")).toBe(true);
    expect(credentialReady(" ", "api", "s")).toBe(false);
    expect(credentialReady("https://crm.example", "", "s")).toBe(false);
    expect(credentialReady("https://crm.example", "api", "")).toBe(false);
  });

  it("judges the secret exactly as typed, without trimming it", () => {
    expect(credentialReady("https://crm.example", "api", " ")).toBe(true);
  });
});
