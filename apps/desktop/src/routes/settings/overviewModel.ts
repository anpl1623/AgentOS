/**
 * The tiles at the top of the settings screen: the four facts an operator
 * would otherwise scroll for, each leading to the section that explains it.
 *
 * Quiet unless something is wrong. An account with no token is a failure —
 * every call made as it fails — and is drawn as one; a scheduler with work it
 * can never start, or that stopped on its own, asks for attention; an audit
 * chain is coloured only once a check of it is running, has failed to run, or
 * has an answer.
 */

import type { IntegrationView } from "../../bindings/IntegrationView";
import type { SchedulerView } from "../../bindings/SchedulerView";
import type { SettingsView } from "../../bindings/SettingsView";
import type { IconName } from "../../components/icons";
import type { StatTone } from "../../components/StatTile";
import { type AuditFindings, verificationTone, verificationWord } from "./auditModel";
import { boundAccounts, brokenAccounts } from "./integrationsModel";
import { nextFiring } from "./schedulerModel";
import { counted, relative } from "./settingsModel";

/** One tile, and the section it leads to. */
export interface OverviewTile {
  /** The id of the section's heading. */
  section: string;
  label: string;
  icon: IconName;
  /** A count, a word such as "Off", or `null` while it is still being read. */
  value: number | string | null;
  hint: string;
  tone: StatTone;
}

/** What the tiles are read from; each is `null` until it has been read. */
export interface OverviewInput {
  settings: SettingsView | null;
  integrations: readonly IntegrationView[] | null;
  scheduler: SchedulerView | null;
  /** The last check of the audit chain this window finished. */
  audit: AuditFindings | null;
  /** A check of the audit chain is running now. */
  checking: boolean;
  /** Why the latest check could not run, until another is started. */
  checkError: string | null;
  now: number;
}

export function overviewTiles(input: OverviewInput): OverviewTile[] {
  return [
    keysTile(input.settings),
    accountsTile(input.integrations),
    schedulerTile(input.scheduler, input.now),
    auditTile(input),
  ];
}

function keysTile(settings: SettingsView | null): OverviewTile {
  const tile = {
    section: "settings-providers",
    label: "Provider keys",
    icon: "key",
  } as const;
  if (settings === null) return { ...tile, value: null, hint: "", tone: "accent" };
  const set = settings.providers.filter((provider) => provider.configured);
  if (!settings.keychain_available) {
    return {
      ...tile,
      value: set.length,
      hint: "No keychain here: keys come from the environment",
      tone: "warn",
    };
  }
  const names = set.length === 0 ? "none yet" : set.map((provider) => provider.id).join(", ");
  return {
    ...tile,
    value: set.length,
    hint: `of ${settings.providers.length} providers: ${names}`,
    tone: "accent",
  };
}

function accountsTile(integrations: readonly IntegrationView[] | null): OverviewTile {
  const tile = { section: "settings-integrations", label: "Accounts", icon: "link" } as const;
  if (integrations === null) return { ...tile, value: null, hint: "", tone: "accent" };
  const bound = boundAccounts(integrations);
  const broken = brokenAccounts(integrations);
  // The loud figure is the one that is wrong: a red count of every account
  // would read as that many failing.
  if (broken > 0) {
    return {
      ...tile,
      value: `${broken} broken`,
      hint: `of ${bound} bound: no token, so ${broken === 1 ? "its" : "their"} calls fail`,
      tone: "danger",
    };
  }
  const used = integrations
    .filter((integration) => integration.accounts.length > 0)
    .map((integration) => integration.display_name);
  return {
    ...tile,
    value: bound,
    hint: used.length === 0 ? "None bound yet" : `on ${used.join(", ")}`,
    tone: "accent",
  };
}

function schedulerTile(view: SchedulerView | null, now: number): OverviewTile {
  const tile = { section: "settings-scheduler", label: "Scheduler", icon: "clock" } as const;
  if (view === null) return { ...tile, value: null, hint: "", tone: "accent" };
  const value = view.running ? "Running" : "Off";
  // What needs a person, most pressing first; the same two counts the panel
  // flags, and a stop nobody asked for.
  const attention = [
    !view.running && view.error ? "Stopped on its own" : null,
    view.overdue > 0 ? `${counted(view.overdue, "schedule", "schedules")} overdue` : null,
    view.unreachable_tasks > 0
      ? `${counted(view.unreachable_tasks, "task", "tasks")} can never start`
      : null,
  ].filter((part): part is string => part !== null);
  if (attention.length > 0) return { ...tile, value, hint: attention.join(" · "), tone: "warn" };
  if (view.running) {
    return { ...tile, value, hint: `Next firing ${nextFiring(view, now)}`, tone: "ok" };
  }
  return {
    ...tile,
    value,
    hint: `${counted(view.active_schedules, "schedule", "schedules")} waiting`,
    tone: "accent",
  };
}

/**
 * The audit chain. The header's primary action starts a check and this tile
 * is where its answer lands, so it says when a check is running and when one
 * could not run, as well as what the last one found; otherwise a failed check
 * would be said only in the audit section, a long scroll below the button.
 * "Checked", never "verified", beside a result: a chain with a break was
 * checked and did not verify.
 */
function auditTile({ audit, checking, checkError, now }: OverviewInput): OverviewTile {
  const tile = { section: "settings-audit", label: "Audit chain", icon: "shield" } as const;
  // "Checked now" reads as an instruction; within the minute it is "just now".
  const when = audit === null ? null : relative(audit.at, now);
  const ago = when === "now" ? "just now" : when;
  const last = ago === null ? null : `last checked ${ago}`;
  if (checking) {
    return {
      ...tile,
      value: "Checking…",
      hint: last === null ? "Recomputing every hash" : `Recomputing every hash; ${last}`,
      tone: "info",
    };
  }
  if (checkError !== null) {
    return {
      ...tile,
      value: "Check failed",
      hint: last === null ? "The check could not run" : `The check could not run; ${last}`,
      tone: "danger",
    };
  }
  if (audit === null) {
    return {
      ...tile,
      value: verificationWord(null),
      hint: "Not checked since the window opened",
      tone: "accent",
    };
  }
  return {
    ...tile,
    value: verificationWord(audit),
    hint: `Checked ${ago}`,
    tone: verificationTone(audit),
  };
}
