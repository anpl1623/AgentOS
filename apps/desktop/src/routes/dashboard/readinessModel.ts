/**
 * What stands between this installation and a first run, and how far along
 * setting it up is.
 *
 * Apart from the rest of the dashboard's model because it reads different
 * answers (settings and the agent list) and is drawn in a place of its own: a
 * progress card above the work, shown only while something is missing.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { SettingsView } from "../../bindings/SettingsView";
import type { Route } from "../route";
import { firstLine } from "./dashboardModel";

/**
 * Providers that run without a key, as `agentos-providers` defines them:
 * Ollama is local and the mock is built in. The settings view reports them as
 * unconfigured, which for these means nothing is missing.
 */
const KEYLESS_PROVIDERS: ReadonlySet<string> = new Set(["ollama", "mock"]);

/** One thing this installation still lacks, and where to fix it. */
export interface ReadinessItem {
  id: "provider" | "keychain" | "agent" | "browser";
  /** What is missing, as a statement. */
  problem: string;
  /** What to do about it. */
  remedy: string;
  /** The screen where it is done. */
  target: Route;
}

/**
 * What stands between this installation and a first run: the desktop's
 * `agentos doctor`.
 *
 * Empty until both settings and the agent list have loaded, so the block never
 * flashes a problem that the second answer would have withdrawn. Empty again
 * once nothing is missing, which is when the dashboard stops showing it.
 *
 * Like the doctor, a missing keychain or browser is a fact about the machine
 * rather than a fault, and is raised only where it stops something: the
 * keychain when there is a key to add and nowhere to keep it, the browser when
 * an enabled agent holds a browser tool or no agent exists yet to say
 * otherwise. Otherwise a headless machine configured from the environment, or
 * an installation with no use for a browser, would carry a warning it can
 * never clear.
 */
export function readiness(
  settings: SettingsView | null,
  agents: readonly AgentSummary[] | null,
): ReadinessItem[] {
  if (settings === null || agents === null) return [];
  const items: ReadinessItem[] = [];
  const toSettings: Route = { name: "settings" };
  const enabled = agents.filter((agent) => agent.status === "enabled");

  const keyed = new Set(
    settings.providers.filter((provider) => provider.configured).map((provider) => provider.id),
  );
  const known = new Set(settings.providers.map((provider) => provider.id));
  // An agent on a provider this build does not list is left alone: the window
  // cannot tell whether it needs a key, and saying it does would be a guess.
  const unkeyed = enabled.filter(
    (agent) =>
      known.has(agent.provider) &&
      !KEYLESS_PROVIDERS.has(agent.provider) &&
      !keyed.has(agent.provider),
  );
  const needsKey = unkeyed.length > 0 || (enabled.length === 0 && keyed.size === 0);
  if (unkeyed.length > 0) {
    const providers = [...new Set(unkeyed.map((agent) => agent.provider))];
    items.push({
      id: "provider",
      problem:
        unkeyed.length === 1
          ? `${unkeyed[0]!.name} uses ${unkeyed[0]!.provider}, which has no key.`
          : `${unkeyed.length} agents use a provider with no key: ${providers.join(", ")}.`,
      remedy: `Add ${providers.length === 1 ? `the ${providers[0]} key` : "their keys"} in Settings.`,
      target: toSettings,
    });
  } else if (needsKey) {
    items.push({
      id: "provider",
      problem: "No model provider has a key.",
      remedy: "Add one in Settings, or give an agent Ollama, which runs locally without one.",
      target: toSettings,
    });
  }

  if (needsKey && !settings.keychain_available) {
    items.push({
      id: "keychain",
      problem: settings.keychain_reason
        ? `The system keychain is unavailable: ${firstLine(settings.keychain_reason)}`
        : "The system keychain is unavailable.",
      remedy:
        "A key cannot be saved from this window. Set it in the environment before opening AgentOS; Settings names the variable.",
      target: toSettings,
    });
  }

  if (agents.length === 0) {
    items.push({
      id: "agent",
      problem: "No agents yet.",
      remedy: "Create one, and grant it only the tools it needs.",
      target: { name: "agents" },
    });
  } else if (enabled.length === 0) {
    items.push({
      id: "agent",
      problem: "Every agent is disabled.",
      remedy: "Enable one before giving it work.",
      target: { name: "agents" },
    });
  }

  if (settings.browser_path === null) {
    const browserTools = new Set(
      settings.tools.filter((tool) => tool.domain === "browser").map((tool) => tool.name),
    );
    const holders = enabled.filter((agent) => agent.tools.some((tool) => browserTools.has(tool)));
    if (holders.length > 0 || agents.length === 0) {
      items.push({
        id: "browser",
        problem:
          holders.length > 0
            ? `No browser found, and ${holders.map((agent) => agent.name).join(", ")} ${holders.length === 1 ? "has" : "have"} browser tools.`
            : "No browser found.",
        remedy:
          settings.browser_hint ??
          "Install Chrome or Chromium; the browser tools fail until one is found.",
        target: toSettings,
      });
    }
  }

  return items;
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/**
 * Every check {@link readiness} makes, in the order a person setting up would
 * meet them, each named for what is true once it is done.
 *
 * The words must hold however the step came to be done, since readiness
 * passes the keychain and the browser where nothing needs them: a machine
 * with no keychain and no key to keep is done, and a tick beside "keys have
 * somewhere safe to live" would be a reassurance about something that is not
 * there. Nor is a provider "reachable" because it has a key; nothing here
 * has called it.
 */
export const SETUP_STEPS: readonly { id: ReadinessItem["id"]; done: string }[] = [
  { id: "provider", done: "No agent is waiting on a provider key" },
  { id: "keychain", done: "Keys can be kept, or none is needed" },
  { id: "agent", done: "An agent is enabled for work" },
  { id: "browser", done: "No agent is missing a browser" },
];

/** One step of the setup card: done, or the item that says what is missing. */
export interface SetupStep {
  id: ReadinessItem["id"];
  /** What the step looks like when done. */
  done: string;
  /** What is missing, or `null` when the step is done. */
  missing: ReadinessItem | null;
}

/** How far setup has come, for the progress card. */
export interface SetupProgress {
  done: number;
  total: number;
  /** Whole percent done, for the bar. */
  percent: number;
  /** Every step, missing ones first so the card leads with what to do. */
  steps: SetupStep[];
}

/**
 * The setup card's figures, from what {@link readiness} found missing.
 *
 * A check that readiness did not raise counts as done, including the two it
 * raises only where they stop something, the keychain and the browser: a
 * machine with no use for either has nothing left to do about it, and a card
 * that could never reach 100% would be a warning nobody can clear.
 */
export function setupProgress(missing: readonly ReadinessItem[]): SetupProgress {
  const byId = new Map(missing.map((item) => [item.id, item] as const));
  const steps: SetupStep[] = SETUP_STEPS.map((step) => ({
    id: step.id,
    done: step.done,
    missing: byId.get(step.id) ?? null,
  }));
  const done = steps.filter((step) => step.missing === null).length;
  const total = steps.length;
  return {
    done,
    total,
    percent: total === 0 ? 100 : Math.round((done / total) * 100),
    steps: [
      ...steps.filter((step) => step.missing !== null),
      ...steps.filter((step) => step.missing === null),
    ],
  };
}
