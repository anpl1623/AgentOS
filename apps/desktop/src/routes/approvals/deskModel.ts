/**
 * What the approvals desk shows, without React: which request the detail pane
 * presents, the figures above the queue, and the lines a card builds from the
 * request — the rule that matched, where untrusted data came from, how much of
 * the run's approval budget is spent.
 *
 * Nothing here decides anything. The rules that keep a decision on the card
 * it was made for are in `approvalsModel.ts`; these only say how the queue is
 * laid out for reading.
 */

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { IconName } from "../../components/icons";
import { compareRisk, riskTone } from "../../components/status";
import type { StatTone } from "../../components/StatTile";
import { humanise } from "../../sdk/format";
import { type Selection, askedOnlyForTaint, blockedFor } from "./approvalsModel";

/**
 * The request the screen presents as the one to read next: the selected card,
 * else the card holding a half-made decision, else the most dangerous card in
 * the queue. `order` is the queue in reading order.
 *
 * Side by side, this is the request in the detail pane; stacked, it is the
 * card that carries the glow. A selection or intent naming a card that has
 * left falls through, so the pane never shows a request that is not waiting.
 * Presenting a card selects nothing: a decision still needs its own presses on
 * that card's buttons.
 */
export function shownId(selection: Selection, order: readonly string[]): string | null {
  const { selected, intent } = selection;
  if (selected !== null && order.includes(selected)) return selected;
  if (intent !== null && order.includes(intent.id)) return intent.id;
  return order[0] ?? null;
}

/** The figures over the queue. */
export interface QueueSummary {
  waiting: number;
  /** Requests from runs that have read untrusted data. */
  tainted: number;
  /** Of those, the ones the policy would have allowed had the run read nothing untrusted. */
  onlyTaint: number;
  /** The most dangerous level waiting, as the runtime wrote it; `null` for an empty queue. */
  highest: string | null;
  /** How long the longest-waiting request has held its run, or `null` for an empty queue. */
  oldest: string | null;
}

export function queueSummary(list: readonly ApprovalView[], now: number): QueueSummary {
  let highest: string | null = null;
  let oldest: ApprovalView | null = null;
  let tainted = 0;
  let onlyTaint = 0;
  for (const approval of list) {
    if (highest === null || compareRisk(approval.risk, highest) > 0) highest = approval.risk;
    if (oldest === null || instant(approval.requested_at) < instant(oldest.requested_at)) {
      oldest = approval;
    }
    if (approval.tainted) tainted += 1;
    if (askedOnlyForTaint(approval)) onlyTaint += 1;
  }
  return {
    waiting: list.length,
    tainted,
    onlyTaint,
    highest,
    oldest: oldest === null ? null : blockedFor(oldest.requested_at, now),
  };
}

function instant(iso: string): number {
  const at = Date.parse(iso);
  return Number.isNaN(at) ? Number.POSITIVE_INFINITY : at;
}

/**
 * The tile tone for the most dangerous level waiting. Quiet for nothing, none
 * and low, because a tile in a status hue is a tile asking for attention.
 */
export function riskStatTone(level: string | null): StatTone {
  if (level === null) return "accent";
  switch (riskTone(level)) {
    case "critical":
      return "critical";
    case "high":
      return "danger";
    case "medium":
      return "warn";
    default:
      return "accent";
  }
}

/** A risk level in sentence case, for the tile that names one. */
export function riskWord(level: string): string {
  const words = humanise(level);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** How full the run's approval budget is, for the meter beside its line. */
export interface BudgetMeter {
  /** From 0 to 1. */
  fill: number;
  /** `spent` once this request is the last the policy lets the run ask for. */
  tone: "calm" | "warn" | "spent";
}

/** `null` when the policy sets no budget, as {@link budgetLine} is. */
export function budgetMeter(asked: number, budget: number | null): BudgetMeter | null {
  if (budget === null) return null;
  if (!(budget > 0) || asked >= budget) return { fill: 1, tone: "spent" };
  const fill = Math.min(1, Math.max(0, asked / budget));
  return { fill, tone: fill >= 0.8 ? "warn" : "calm" };
}

/**
 * The id of the policy rule a reason names, when it opens the way the
 * permission engine writes one: "rule `browser.interact` matched: …". `null`
 * for a reason in any other form, which the card then shows only as written.
 */
export function matchedRule(reason: string): string | null {
  const match = /^rule `([^`]+)`/.exec(reason);
  return match?.[1] ?? null;
}

/**
 * What the policy would have done without the taint, said beside the rule.
 * `null` for an effect this build does not know, rather than a guess.
 */
export function ruleEffect(approval: ApprovalView): string | null {
  if (askedOnlyForTaint(approval)) return "allows this; the untrusted read made it a question";
  if (approval.effect_before_taint === "ask") return "asks a person for this";
  return null;
}

/** Where a run's untrusted data came from, split for drawing. */
export interface TaintOrigin {
  /** The source's kind as the runtime labels it, such as `web` or `file`; `null` when it has none. */
  kind: string | null;
  /** Everything after the kind, exactly as written. */
  detail: string;
  icon: IconName;
}

const ORIGIN_ICONS: Readonly<Record<string, IconName>> = {
  web: "globe",
  file: "folder",
  terminal: "terminal",
  screen: "window",
  tool: "zap",
  agent: "agents",
};

/**
 * A taint source label (`web:https://…`, `file:/work/a`, an integration's
 * `github:acct@/repos/x`) as a kind and the rest. The kind is only taken when
 * the label opens with a short identifier and a colon, so a source in any
 * other shape is drawn whole rather than cut somewhere arbitrary.
 */
export function taintOrigin(source: string): TaintOrigin {
  const match = /^([a-z][a-z0-9_.-]{0,31}):(.+)$/is.exec(source);
  if (match === null) return { kind: null, detail: source, icon: "link" };
  const kind = match[1] ?? "";
  const detail = match[2] ?? "";
  const key = kind.toLowerCase();
  return {
    kind,
    detail,
    icon: Object.hasOwn(ORIGIN_ICONS, key) ? (ORIGIN_ICONS[key] ?? "link") : "link",
  };
}

const TOOL_ICONS: Readonly<Record<string, IconName>> = {
  browser: "window",
  computer: "monitor",
  email: "external",
  filesystem: "folder",
  network: "globe",
  terminal: "terminal",
  runtime: "cpu",
};

/**
 * The mark beside a request, from its tool's namespace (`browser.type` is a
 * browser action), so a queue is scanned by what kind of thing each request
 * would touch. A namespace this build does not know gets the generic action
 * mark rather than a guess.
 */
export function toolIcon(tool: string): IconName {
  const namespace = tool.split(".", 1)[0] ?? "";
  return Object.hasOwn(TOOL_ICONS, namespace) ? (TOOL_ICONS[namespace] ?? "zap") : "zap";
}

/**
 * The name of a request in the side-by-side queue, read in place of the
 * item's drawn text: the risk chip's and the taint mark's hidden words would
 * otherwise make every item a paragraph. It ends with what the item's marks
 * say about the operator's own dealings with it: a decision half-made on it,
 * or an answer that did not go through, whose reason is on its card.
 */
export function queueItemLabel(
  approval: ApprovalView,
  now: number,
  marks: { armed?: boolean; failed?: boolean } = {},
): string {
  const taint = approval.tainted ? ", read untrusted data" : "";
  const armed = marks.armed === true ? " A decision is half-made on it." : "";
  const failed = marks.failed === true ? " Your last answer to it did not go through." : "";
  return `${approval.agent_name} wants to run ${approval.tool}. ${approval.risk} risk${taint}. Blocked for ${blockedFor(approval.requested_at, now)}.${armed}${failed}`;
}

/** How the recently decided requests went. */
export interface Tally {
  approved: number;
  denied: number;
  other: number;
}

export function recentTally(list: readonly Pick<ApprovalView, "status">[]): Tally {
  const tally = { approved: 0, denied: 0, other: 0 };
  for (const { status } of list) {
    if (status === "approved") tally.approved += 1;
    else if (status === "denied") tally.denied += 1;
    else tally.other += 1;
  }
  return tally;
}

/** "2 approved · 1 denied", leaving out what did not happen. */
export function tallyLine(tally: Tally): string {
  const parts = [
    tally.approved > 0 ? `${tally.approved} approved` : null,
    tally.denied > 0 ? `${tally.denied} denied` : null,
    tally.other > 0 ? `${tally.other} other` : null,
  ].filter((part): part is string => part !== null);
  return parts.length === 0 ? "none yet" : parts.join(" · ");
}
