/**
 * The approvals screen's rules, without React.
 *
 * Two things are decided here: the order the cards are read in, and which
 * card a half-made decision belongs to. The second is the dangerous one. The
 * queue changes underneath the operator — cards arrive by event, and a decided
 * card leaves before the runtime has even answered — so anything that names a
 * card by its place in the list will, sooner or later, name the wrong card.
 * Every intent here names its card by id, and lapses when that card leaves.
 */

import type { ApprovalView } from "../bindings/ApprovalView";
import { countHidden } from "../components/argumentText";
import { compareRisk, riskTone } from "../components/status";

function instant(iso: string): number {
  const at = Date.parse(iso);
  return Number.isNaN(at) ? Number.POSITIVE_INFINITY : at;
}

/**
 * The cards in the order they should be read: most dangerous first, and
 * within a level the one that has waited longest.
 *
 * The runtime lists oldest first, which puts a critical request behind every
 * low-risk one raised before it. A level this build does not know sorts above
 * critical, for the reason `compareRisk` gives. A time that cannot be read
 * sorts last within its level rather than jumping the queue.
 */
export function sortApprovals(list: readonly ApprovalView[]): ApprovalView[] {
  return [...list].sort(
    (a, b) =>
      compareRisk(b.risk, a.risk) ||
      instant(a.requested_at) - instant(b.requested_at) ||
      (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

/**
 * How long the run has been stopped waiting, to the minute.
 *
 * Minutes, not seconds, and rounded down: the figure is the cost of not
 * deciding, stated once, and a number that ticks every second puts a clock on
 * the one decision in the product that is meant to be made slowly.
 */
export function blockedFor(requestedAt: string, now: number): string {
  const at = Date.parse(requestedAt);
  if (Number.isNaN(at)) return "an unknown time";
  const minutes = Math.floor(Math.max(0, now - at) / 60_000);
  if (minutes < 1) return "under a minute";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 === 0 ? `${hours}h` : `${hours}h ${minutes % 60}m`;
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days}d` : `${days}d ${hours % 24}h`;
}

/** The run's use of its approval budget, or `null` when the policy sets none. */
export function budgetLine(asked: number, budget: number | null): string | null {
  return budget === null ? null : `asked ${asked} of ${budget} times this run`;
}

/**
 * Whether the policy would have let this run without asking, so taint alone
 * is why a person is being asked. The card says so, because "the rules allow
 * this, but the agent has read something it should not trust" and "the rules
 * want a person for this" call for different readings.
 */
export function askedOnlyForTaint(approval: ApprovalView): boolean {
  return approval.tainted && approval.effect_before_taint === "allow";
}

/**
 * What to announce when requests arrive, and the ids seen so far.
 *
 * Arrivals are announced as one line rather than by making the card list a
 * live region: a card is its explanation, its facts, every argument and its
 * buttons, and a screen reader would queue all of it for speech. `seen` is
 * `null` until the queue first loads, and what is there then is the queue,
 * not news. A request seen before and back again, as one is when its answer
 * could not be delivered, is not announced twice. `list` is in reading order,
 * so the first new card named is the most dangerous.
 */
export function arrivals(
  seen: ReadonlySet<string> | null,
  list: readonly ApprovalView[],
): { seen: ReadonlySet<string>; notice: string | null } {
  const next = new Set(seen ?? []);
  const fresh: ApprovalView[] = [];
  for (const approval of list) {
    if (!next.has(approval.id)) {
      next.add(approval.id);
      if (seen !== null) fresh.push(approval);
    }
  }
  const [first] = fresh;
  if (first === undefined) return { seen: next, notice: null };
  const what = `${first.agent_name} wants to run ${first.tool}, ${first.risk} risk.`;
  return {
    seen: next,
    notice:
      fresh.length === 1
        ? `New request: ${what}`
        : `${fresh.length} new requests. The first: ${what}`,
  };
}

/**
 * The card's own text that is built from what the model sent: the tool's plan
 * summary, the reason and the resources it names, and the sources of taint.
 * Each can carry the characters the argument block escapes, and is drawn the
 * same way.
 */
export function describedText(
  approval: Pick<
    ApprovalView,
    "explanation" | "objective" | "reason" | "affected_resources" | "taint_sources"
  >,
): string[] {
  return [
    approval.explanation,
    approval.objective,
    approval.reason,
    ...approval.affected_resources,
    ...approval.taint_sources,
  ];
}

/**
 * The warning above a card whose description holds invisible or
 * direction-changing characters, or `null` when it holds none.
 *
 * Separate from the argument block's own count: the summary is the first
 * thing read, and it is where a reordered file name or command would be
 * believed before the arguments below it are reached.
 */
export function describedWarning(approval: Parameters<typeof describedText>[0]): string | null {
  const count = describedText(approval).reduce((sum, text) => sum + countHidden(text), 0);
  if (count === 0) return null;
  return count === 1
    ? "This request's description holds 1 invisible or direction-changing character, shown as ⟨U+…⟩. It may not say what it reads as saying."
    : `This request's description holds ${count} invisible or direction-changing characters, shown as ⟨U+…⟩. It may not say what it reads as saying.`;
}

/**
 * Whether to offer denying the call and stopping the run together.
 *
 * Offered when the run is tainted and for high or critical risk. Refusing one
 * call leaves the run reasoning from the context that produced it, and in
 * those cases that context is the problem.
 */
export function offersStop(approval: ApprovalView): boolean {
  const tone = riskTone(approval.risk);
  return approval.tainted || tone === "high" || tone === "critical";
}

/** A decided request's status as a verdict tone. */
export function decisionTone(status: string): "ok" | "blocked" | "neutral" {
  if (status === "approved") return "ok";
  if (status === "denied") return "blocked";
  return "neutral";
}

/**
 * Whether a click is one a person made on purpose.
 *
 * The second and third clicks of a double or triple click are not. Without
 * this, a double click on Approve arms it and confirms it in one gesture,
 * because Confirm is drawn where Approve was; and after a confirm the next
 * card slides up under the pointer, so a triple click would arm and confirm a
 * card nobody read. A click from the keyboard has a `detail` of 0.
 */
export function isDeliberate(detail: number): boolean {
  return detail <= 1;
}

// ---------------------------------------------------------------------------
// Selection and intent
// ---------------------------------------------------------------------------

/** What a confirm would do. `stop` is a denial that also stops the run. */
export type IntentKind = "approve" | "deny" | "stop";

/** A half-made decision: which card, which way, and the note so far. */
export interface Intent {
  id: string;
  kind: IntentKind;
  note: string;
}

/**
 * The keyboard's place in the queue and the one decision being made.
 *
 * At most one card holds an intent. Arming another card disarms the first, so
 * there is never a second confirm waiting anywhere on the page.
 */
export interface Selection {
  selected: string | null;
  intent: Intent | null;
}

/** Nothing selected, nothing armed. */
export const idleSelection: Selection = { selected: null, intent: null };

/** What can happen to a {@link Selection}. */
export type SelectionAction =
  /** `j`/`k` and the arrows: move through `order`, the cards as drawn. */
  | { kind: "move"; by: 1 | -1; order: readonly string[] }
  /** Focus or a deep link landed on a card. */
  | { kind: "select"; id: string }
  /** Approve, Deny or Deny and stop was pressed on a card. */
  | { kind: "intend"; id: string; intent: IntentKind }
  /** The note was edited. Ignored unless that card holds the intent. */
  | { kind: "note"; id: string; text: string }
  /** Cancel or Escape. */
  | { kind: "cancel" }
  /** The operator confirmed a decision. */
  | { kind: "resolved" }
  /** The queue changed; `ids` is every card now shown. */
  | { kind: "queue"; ids: readonly string[] };

/** Apply one action. */
export function nextSelection(state: Selection, action: SelectionAction): Selection {
  switch (action.kind) {
    case "move": {
      const { order, by } = action;
      if (order.length === 0) return { ...state, selected: null };
      const at = state.selected === null ? -1 : order.indexOf(state.selected);
      const index =
        at === -1
          ? by === 1
            ? 0
            : order.length - 1
          : Math.min(order.length - 1, Math.max(0, at + by));
      return { ...state, selected: order[index] ?? null };
    }
    case "select":
      return state.selected === action.id ? state : { ...state, selected: action.id };
    case "intend":
      return { selected: action.id, intent: { id: action.id, kind: action.intent, note: "" } };
    case "note":
      return state.intent?.id === action.id
        ? { ...state, intent: { ...state.intent, note: action.text } }
        : state;
    case "cancel":
      return state.intent === null ? state : { ...state, intent: null };
    case "resolved":
      // Everything goes back to idle, not to the neighbouring card: the card
      // that slides into the decided card's place has not been read yet.
      return idleSelection;
    case "queue": {
      const present = new Set(action.ids);
      const selected =
        state.selected !== null && present.has(state.selected) ? state.selected : null;
      const intent = state.intent !== null && present.has(state.intent.id) ? state.intent : null;
      return selected === state.selected && intent === state.intent ? state : { selected, intent };
    }
  }
}
