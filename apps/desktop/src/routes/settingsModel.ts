/**
 * What the settings screen decides, apart from how it draws it.
 *
 * Who holds each tool, which tools a filter keeps, what the scheduler panel
 * claims about the work it would find, and how a verification, a stored
 * credential and a bound account are worded all live here as pure functions,
 * so a test fails when one of them changes. `Settings.tsx` only renders what
 * these return.
 */

import type { AgentSummary } from "../bindings/AgentSummary";
import type { IntegrationAccountView } from "../bindings/IntegrationAccountView";
import type { IntegrationTestView } from "../bindings/IntegrationTestView";
import type { IntegrationView } from "../bindings/IntegrationView";
import type { NetworkCredentialView } from "../bindings/NetworkCredentialView";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { ToolView } from "../bindings/ToolView";
import { compareRisk } from "../components/status";

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/**
 * A moment relative to `now`, either side of it: `in 4m`, `2h ago`.
 *
 * Whole units, rounded towards zero, so a schedule due in eleven and a half
 * minutes is never promised in twelve. Within a minute either way it is `now`,
 * and a time that cannot be read says so rather than printing `NaN`.
 */
export function relative(iso: string, now: number): string {
  const offset = Date.parse(iso) - now;
  if (!Number.isFinite(offset)) return "at an unreadable time";
  const length = Math.abs(offset);
  if (length < 60_000) return "now";
  const minutes = Math.floor(length / 60_000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  const amount = days > 0 ? `${days}d` : hours > 0 ? `${hours}h` : `${minutes}m`;
  return offset > 0 ? `in ${amount}` : `${amount} ago`;
}

/**
 * A runtime message as a sentence: first letter capitalised, closing stop.
 *
 * The runtime writes its refusals to follow a colon, in lower case. Shown on
 * their own in a banner, they read better as a sentence; the words are left
 * exactly as written.
 */
export function asSentence(message: string): string {
  const trimmed = message.trim();
  if (trimmed === "") return trimmed;
  const capitalised = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capitalised) ? capitalised : `${capitalised}.`;
}

// ---------------------------------------------------------------------------
// Tool catalogue
// ---------------------------------------------------------------------------

/** An agent that has been granted a tool. */
export interface Holder {
  id: string;
  name: string;
  /** A disabled agent keeps its grants; enabling it gives them back. */
  enabled: boolean;
}

/** One tool, with who on this machine holds it. */
export interface CatalogueEntry {
  tool: ToolView;
  /**
   * The agents granted it, by name. `null` when the agents could not be read:
   * "no agent" would then be a claim nobody checked.
   */
  holders: Holder[] | null;
}

/** The tools of one domain, most dangerous first. */
export interface CatalogueGroup {
  domain: string;
  entries: CatalogueEntry[];
}

/** The domain a tool belongs to, from its view or else from its name. */
function domainOf(tool: ToolView): string {
  if (tool.domain !== "") return tool.domain;
  const dot = tool.name.indexOf(".");
  return dot === -1 ? tool.name : tool.name.slice(0, dot);
}

/**
 * The agents granted `tool`, ordered by name.
 *
 * Disabled agents are included. A grant is a standing fact about the agent,
 * not about whether it is running today, and a disabled agent holding
 * `terminal.exec` is one switch away from using it.
 */
export function holdersOf(tool: string, agents: readonly AgentSummary[]): Holder[] {
  return agents
    .filter((agent) => agent.tools.includes(tool))
    .map((agent) => ({ id: agent.id, name: agent.name, enabled: agent.status === "enabled" }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/**
 * The filter as lower-case terms. Every term must match somewhere in an entry,
 * so `filesystem high` narrows rather than widens.
 */
export function filterTerms(filter: string): string[] {
  return filter.toLowerCase().split(/\s+/).filter((term) => term !== "");
}

/**
 * Whether an entry is kept by the filter.
 *
 * A term may match the tool's name, domain, description, risk, a declared
 * capability, or the name of an agent that holds it, so both "who can run
 * `terminal.exec`?" and "what does `sales` hold?" are one word in the box.
 */
export function entryMatches(entry: CatalogueEntry, terms: readonly string[]): boolean {
  if (terms.length === 0) return true;
  const { tool } = entry;
  const haystack = [
    tool.name,
    tool.domain,
    tool.description,
    tool.risk,
    ...tool.capabilities,
    ...(tool.returns_untrusted_data ? ["external"] : []),
    ...(entry.holders ?? []).map((holder) => holder.name),
  ]
    .join("\n")
    .toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

/**
 * The catalogue grouped by domain, filtered, with each tool's holders.
 *
 * Domains are alphabetical. Within one, the most dangerous tool comes first,
 * because the question this screen answers is who holds the dangerous ones;
 * ties go by name. A domain the filter empties is dropped rather than shown
 * as a heading over nothing.
 */
export function catalogue(
  tools: readonly ToolView[],
  agents: readonly AgentSummary[] | null,
  filter: string,
): CatalogueGroup[] {
  const terms = filterTerms(filter);
  const groups = new Map<string, CatalogueEntry[]>();
  for (const tool of tools) {
    const entry: CatalogueEntry = {
      tool,
      holders: agents === null ? null : holdersOf(tool.name, agents),
    };
    if (!entryMatches(entry, terms)) continue;
    const domain = domainOf(tool);
    const entries = groups.get(domain);
    if (entries) entries.push(entry);
    else groups.set(domain, [entry]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([domain, entries]) => ({
      domain,
      entries: entries.sort(
        (a, b) => compareRisk(b.tool.risk, a.tool.risk) || a.tool.name.localeCompare(b.tool.name),
      ),
    }));
}

/** How many tools a grouped catalogue holds. */
export function countEntries(groups: readonly CatalogueGroup[]): number {
  return groups.reduce((total, group) => total + group.entries.length, 0);
}

/** The holders line of a catalogue row, and whether it names nobody. */
export interface GrantLine {
  text: string;
  /** Nobody holds it, which is drawn quieter than a list of names. */
  nobody: boolean;
}

/**
 * Who holds a tool, as the row says it.
 *
 * A disabled holder is named as one, so a grant nobody is using today is not
 * mistaken for one that is live, nor hidden as if it were gone.
 */
export function grantLine(holders: readonly Holder[] | null): GrantLine {
  if (holders === null) return { text: "who holds it could not be read", nobody: true };
  if (holders.length === 0) return { text: "granted to no agent", nobody: true };
  const names = holders.map((holder) =>
    holder.enabled ? holder.name : `${holder.name} (disabled)`,
  );
  return { text: `granted to ${names.join(", ")}`, nobody: false };
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/**
 * A count typed into a number field, or `null` if it is not a whole number.
 *
 * Only the shape is checked. Whether five seconds is too short a tick is the
 * runtime's to say, and its refusal is shown as written; a second copy of
 * that rule here would one day disagree with the first.
 */
export function parseCount(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) ? value : null;
}

/** The pacing fields as typed, read against the scheduler's own. */
export interface Pacing {
  tick: number | null;
  max: number | null;
  /** Both fields hold whole numbers, so the pacing can be sent. */
  valid: boolean;
  /** The fields say something other than the scheduler's current pacing. */
  changed: boolean;
}

/** Read the pacing fields. Text that is not a whole number counts as changed. */
export function readPacing(view: SchedulerView, tickText: string, maxText: string): Pacing {
  const tick = parseCount(tickText);
  const max = parseCount(maxText);
  return {
    tick,
    max,
    valid: tick !== null && max !== null,
    changed: tick !== view.tick_seconds || max !== view.max_concurrent_runs,
  };
}

/**
 * One sentence on what the scheduler is doing.
 *
 * Off says what is kept and what does not happen, because "off" alone leaves
 * an operator to guess whether turning it off threw their schedules away.
 */
export function schedulerSummary(view: SchedulerView, now: number): string {
  if (!view.running) {
    return "Off. Schedules are kept, but nothing fires and no queued task starts.";
  }
  const since = view.started_at ? `, started ${relative(view.started_at, now)}` : "";
  const runs = view.max_concurrent_runs === 1 ? "one run" : `${view.max_concurrent_runs} runs`;
  return (
    `Running${since}. It looks for due work every ${view.tick_seconds}s, ` +
    `with up to ${runs} at once.`
  );
}

/** One line of the scheduler's counts. */
export interface SchedulerFact {
  label: string;
  value: string;
  /** Something an operator should look at, drawn in the attention tone. */
  attention: boolean;
}

/**
 * When the soonest schedule fires.
 *
 * A moment already passed is "due" rather than "in": with the scheduler
 * running it fires on the next tick, and with it off it waits, which the
 * summary beside it already says.
 */
function nextFiring(view: SchedulerView, now: number): string {
  if (view.next_fire_at === null) return "nothing scheduled";
  const at = Date.parse(view.next_fire_at);
  if (at > now) return relative(view.next_fire_at, now);
  return view.running ? "due now" : `due ${relative(view.next_fire_at, now)}`;
}

/**
 * The work the scheduler would find, as label and value pairs.
 *
 * Overdue schedules and tasks that can never start are flagged only when
 * there are some. A stopped scheduler with schedules already due is drawn
 * as needing attention, which is the state these counts exist to show.
 */
export function schedulerFacts(view: SchedulerView, now: number): SchedulerFact[] {
  return [
    { label: "Active schedules", value: String(view.active_schedules), attention: false },
    { label: "Next firing", value: nextFiring(view, now), attention: false },
    { label: "Overdue", value: String(view.overdue), attention: view.overdue > 0 },
    { label: "Ready to start", value: String(view.runnable_tasks), attention: false },
    {
      label: "Can never start",
      value: String(view.unreachable_tasks),
      attention: view.unreachable_tasks > 0,
    },
  ];
}

/**
 * What went wrong with the scheduler, each said once.
 *
 * A switch refused because another process holds the lease is reported twice
 * by the runtime: as the call's refusal and as the status view's `error`.
 * They are the same words, so the second is dropped. A refusal the status
 * does not repeat, such as pacing it would not accept, is still shown, as is
 * a stop the scheduler came to on its own while nobody pressed anything.
 */
export function schedulerNotices(view: SchedulerView | null, refusal: string | null): string[] {
  const notices: string[] = [];
  if (refusal !== null) notices.push(asSentence(refusal));
  const stopped = view?.running === false ? view.error : null;
  if (stopped && !notices.includes(asSentence(stopped))) notices.push(asSentence(stopped));
  return notices;
}

// ---------------------------------------------------------------------------
// Audit verification
// ---------------------------------------------------------------------------

/** A finished verification of the whole chain, as the screen remembers it. */
export interface Verification {
  /** When it finished, as an ISO time. */
  at: string;
  /** Every break found; empty when the chain is intact. */
  problems: string[];
}

function problemCount(count: number): string {
  return count === 1 ? "1 problem" : `${count} problems`;
}

/**
 * The visible line under the button: when the chain was last checked, and
 * what was found. Never checked this session says so, so the absence of a
 * result is not taken for a clean one.
 */
export function verificationLine(last: Verification | null, now: number): string {
  if (last === null) return "not verified since the window opened";
  const found = last.problems.length === 0 ? "intact" : problemCount(last.problems.length);
  return `verified ${relative(last.at, now)} · ${found}`;
}

/** The sentence a screen reader hears when a verification finishes. */
export function verificationAnnouncement(problems: readonly string[]): string {
  return problems.length === 0
    ? "Audit chain verified intact."
    : `Audit chain verification found ${problemCount(problems.length)}.`;
}

// ---------------------------------------------------------------------------
// Network credentials
// ---------------------------------------------------------------------------

/** A stored credential as the runtime lists it: where it may go, and its name. */
export type StoredCredential = NetworkCredentialView;

/** One row of the credentials list. */
export interface CredentialRow {
  /** Stable across reloads, and unambiguous whatever the two parts contain. */
  key: string;
  origin: string;
  name: string;
  /** `{origin} / {name}`, which is everything the row ever shows of the secret. */
  label: string;
  /**
   * The integration account whose token this is, such as `GitHub account
   * work`, or `null`. Removing or replacing the credential removes or
   * replaces that account's token, and the screen says so first.
   */
  account: string | null;
}

/**
 * The credentials as rows, by origin and then name.
 *
 * Built field by field from the origin and the name and nothing else. Should
 * a later view carry more, a value or a hint of one, it still never reaches
 * the screen: a masked secret tells whoever is looking over a shoulder its
 * length and its first characters, which is more than this list has any need
 * to show.
 */
export function credentialRows(stored: readonly StoredCredential[]): CredentialRow[] {
  return stored
    .map(({ origin, name, account }) => ({
      key: JSON.stringify([origin, name]),
      origin,
      name,
      label: `${origin} / ${name}`,
      account: account ?? null,
    }))
    .sort((a, b) => a.origin.localeCompare(b.origin) || a.name.localeCompare(b.name));
}

/**
 * Whether the add form can be sent.
 *
 * Origin and name are trimmed for the check. The secret is not: it is sent
 * exactly as typed, and trimming it here would judge a value other than the
 * one that will be stored.
 */
export function credentialReady(origin: string, name: string, secret: string): boolean {
  return origin.trim() !== "" && name.trim() !== "" && secret !== "";
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

/**
 * What an account label may be: it ends the key its token is stored under,
 * and a policy and a call name the account by it, so the alphabet is small.
 * The runtime holds the same rule and refuses for itself; this copy only
 * decides whether the form can be sent, and says why not as it is typed.
 */
const ACCOUNT_LABEL = /^[a-z0-9-]{1,32}$/;

/** Why a typed label cannot be used, or `null` when it can or nothing is typed yet. */
export function labelProblem(label: string): string | null {
  const trimmed = label.trim();
  if (trimmed === "" || ACCOUNT_LABEL.test(trimmed)) return null;
  return "A label is 1 to 32 lower-case letters, digits or hyphens.";
}

/**
 * Whether the bind form can be sent.
 *
 * The token is judged trimmed, as the command judges it: unlike a network
 * credential's secret, a token is trimmed before it is stored, so one of only
 * spaces would be refused and the button should not offer to send it.
 */
export function bindReady(label: string, token: string): boolean {
  const trimmed = label.trim();
  return trimmed !== "" && ACCOUNT_LABEL.test(trimmed) && token.trim() !== "";
}

/** The host as the command takes it: `null` for the integration's default. */
export function hostArgument(host: string): string | null {
  const trimmed = host.trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * Whether the form offers the private-network permission for `host`.
 *
 * Only for a host other than the integration's own: that one is on the public
 * internet, so the permission could only widen where its token may go, and the
 * runtime refuses it there. The comparison is rough, trimmed and lower-cased
 * with a trailing `/` dropped; the runtime's is exact, and refuses for itself.
 */
export function offersPrivateNetwork(host: string, defaultHost: string): boolean {
  const typed = hostArgument(host);
  const plain = (url: string) => url.trim().toLowerCase().replace(/\/+$/, "");
  return typed !== null && plain(typed) !== plain(defaultHost);
}

/** One row of an integration's account list. */
export interface AccountRow {
  key: string;
  id: string;
  label: string;
  /**
   * The origin its token is stored for, which is where the Network
   * credentials section lists it, or `null` for a host that reads as none.
   */
  origin: string | null;
  /** The token is in the keychain. A row without one is drawn as broken. */
  present: boolean;
  /** Host, private network, the operator's note and last use, in that order. */
  meta: string[];
}

/**
 * The accounts as rows, by label.
 *
 * Built field by field, as the credential rows are, so a view that one day
 * carried a hint of the token would still not draw it. The private-network
 * permission is named whenever it is on: it is the one thing on the row that
 * widens where a request may go.
 */
export function accountRows(
  accounts: readonly IntegrationAccountView[],
  now: number,
): AccountRow[] {
  return accounts
    .map((account) => ({
      key: account.id,
      id: account.id,
      label: account.label,
      origin: account.origin,
      present: account.credential_present,
      meta: [
        account.host,
        ...(account.private_network ? ["private network allowed"] : []),
        ...(account.scopes !== null && account.scopes.trim() !== ""
          ? [`noted: ${account.scopes.trim()}`]
          : []),
        account.last_used_at === null
          ? "never used"
          : `last used ${relative(account.last_used_at, now)}`,
      ],
    }))
    .sort((a, b) => a.label.localeCompare(b.label) || a.id.localeCompare(b.id));
}

/** A finished connection test, as the row shows it. */
export interface TestLine {
  tone: "ok" | "warn" | "danger" | "neutral";
  verdict: string;
  detail: string;
}

/**
 * How a connection test reads.
 *
 * Unauthorised and unreachable are failures of the account, a wrong host one
 * of what was typed when it was bound; all three mean a run acting as it will
 * fail, so none is drawn as quietly as success. An outcome this screen does not
 * know is shown as the runtime named it rather than guessed at.
 */
export function testLine(result: IntegrationTestView): TestLine {
  const detail = asSentence(result.detail);
  switch (result.outcome) {
    case "reachable":
      return { tone: "ok", verdict: "Reachable", detail };
    case "unauthorised":
      return { tone: "danger", verdict: "Unauthorised", detail };
    case "wrong_host":
      return { tone: "warn", verdict: "Wrong host", detail };
    case "unreachable":
      return { tone: "danger", verdict: "Unreachable", detail };
    default:
      return { tone: "neutral", verdict: result.outcome, detail };
  }
}

/** How many accounts are bound across every integration. */
export function boundAccounts(integrations: readonly IntegrationView[]): number {
  return integrations.reduce((total, integration) => total + integration.accounts.length, 0);
}
