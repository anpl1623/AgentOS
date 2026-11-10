/**
 * The approvals screen's rules, without React.
 *
 * Three things are decided here: the order the cards are read in, which card
 * is on screen to be read, and which card a half-made decision belongs to.
 * The last two are the dangerous ones. The queue changes underneath the
 * operator — cards arrive by event, and a decided card leaves before the
 * runtime has even answered — so anything that names a card by its place in
 * the list will, sooner or later, name the wrong card. Every intent here
 * names its card by id, and lapses when that card leaves; and the card on
 * screen is held by id, so the list re-sorting cannot change what is read.
 */

import type { ApprovalView } from "../../bindings/ApprovalView";
import { countHidden } from "../../components/argumentText";
import { compareRisk, riskTone } from "../../components/status";

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
 * Who asked, when the asking run was started by another agent rather than by
 * a person: the asking agent, then each agent above it up to the root.
 *
 * `["supervisor", "worker"]` reads "worker, asked by supervisor", as the
 * runtime's `ApprovalRequest::attribution` writes it for the terminal, so
 * the two clients name a chain alike. `null` for a run a person started,
 * whose chain is empty. The chain is the runtime's,
 * stamped on the request before it was stored, so the card says who chose
 * this work without the operator opening a single trace; and every agent
 * above the asking one holds a policy that also had to let the call this far.
 */
export function chainLine(chain: readonly string[]): string | null {
  if (chain.length < 2) return null;
  return [...chain].reverse().join(", asked by ");
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

/**
 * The longest note a decision keeps, in UTF-16 code units, which is what an
 * input's `maxLength` counts. The audit log stores the note beside the
 * decision, and a field that accepted more than it kept would lose the end of
 * what the operator wrote without telling them.
 */
export const NOTE_LIMIT = 2000;

/** The note's length against {@link NOTE_LIMIT}, and whether it is close. */
export function noteCount(text: string): { line: string; near: boolean } {
  const used = Math.min(text.length, NOTE_LIMIT);
  return {
    line: `${used.toLocaleString("en")} / ${NOTE_LIMIT.toLocaleString("en")}`,
    near: used >= NOTE_LIMIT * 0.9,
  };
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
 * The keyboard's place in the queue, the request on screen, and the one
 * decision being made.
 *
 * At most one card holds an intent. Arming another card disarms the first, so
 * there is never a second confirm waiting anywhere on the page.
 */
export interface Selection {
  selected: string | null;
  intent: Intent | null;
  /**
   * The request last put on screen, held while nothing is selected so that
   * what the operator is reading stays put; see {@link shownId}. Whenever a
   * card is selected, this is that card.
   */
  presented: string | null;
}

/** Nothing selected, nothing armed, nothing yet on screen. */
export const idleSelection: Selection = { selected: null, intent: null, presented: null };

/**
 * The request the screen presents as the one to read: the selected card, else
 * the card holding a half-made decision, else the card already on screen,
 * else the most dangerous card in the queue. `order` is the queue in reading
 * order.
 *
 * Side by side, this is the request in the detail pane, the only one drawn in
 * full; stacked, it is the card that carries the glow. A card named here that
 * has left the queue falls through, so the pane never shows a request that is
 * not waiting.
 *
 * The card on screen is held because the queue re-sorts underneath the
 * operator. Were the most dangerous card always presented, a request arriving
 * at a higher risk, or one coming back after its answer failed, would take
 * over the pane while another was being read, with its own Approve drawn
 * exactly where the one under the pointer had been. So the top of the queue is
 * only presented when nothing has been yet or the card on screen has left;
 * otherwise the newcomer is announced and waits its turn in the queue, and
 * only the operator moving to it puts it on screen.
 *
 * Presenting a card selects nothing: a decision still needs its own presses on
 * that card's buttons.
 */
export function shownId(selection: Selection, order: readonly string[]): string | null {
  const { selected, intent, presented } = selection;
  if (selected !== null && order.includes(selected)) return selected;
  if (intent !== null && order.includes(intent.id)) return intent.id;
  if (presented !== null && order.includes(presented)) return presented;
  return order[0] ?? null;
}

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
  /** An answer did not go through, and its request is waiting again. */
  | { kind: "returned"; id: string }
  /** The queue changed; `ids` is every card now waiting, in reading order. */
  | { kind: "queue"; ids: readonly string[] };

/**
 * Apply one action.
 *
 * Only the operator's own acts change the request on screen — selecting a
 * request, moving to one, or pressing a decision on one — and that request
 * leaving the queue. Nothing that arrives does. See {@link shownId}.
 */
export function nextSelection(state: Selection, action: SelectionAction): Selection {
  switch (action.kind) {
    case "move": {
      const { order, by } = action;
      if (order.length === 0) return { ...state, selected: null };
      const at = state.selected === null ? -1 : order.indexOf(state.selected);
      // With nothing selected, the first press takes hold of the request on
      // screen rather than jumping to an end of the queue: the keys move on
      // from what is being read, and a request that arrived above it while it
      // was read is reached by moving up, like any other.
      const held = state.presented === null ? -1 : order.indexOf(state.presented);
      const index =
        at !== -1
          ? Math.min(order.length - 1, Math.max(0, at + by))
          : held !== -1
            ? held
            : by === 1
              ? 0
              : order.length - 1;
      const selected = order[index] ?? null;
      return { ...state, selected, presented: selected ?? state.presented };
    }
    case "select":
      return state.selected === action.id && state.presented === action.id
        ? state
        : { ...state, selected: action.id, presented: action.id };
    case "intend":
      return {
        selected: action.id,
        intent: { id: action.id, kind: action.intent, note: "" },
        presented: action.id,
      };
    case "note":
      return state.intent?.id === action.id
        ? { ...state, intent: { ...state.intent, note: action.text } }
        : state;
    case "cancel":
      return state.intent === null ? state : { ...state, intent: null };
    case "resolved":
      // Nothing stays selected or armed, and nothing passes to the
      // neighbouring card: the card that slides into the decided card's place
      // has not been read yet. The decided card stays the one on screen until
      // the queue says it has left, so no other request is put in its place
      // before then.
      return { ...idleSelection, presented: state.presented };
    case "returned":
      // The request comes back selected, so the side-by-side desk presents it
      // with the reason its answer failed rather than leaving that reason on
      // a card out of sight. Only while nothing else has the operator's eye: a
      // card they have chosen since, a decision they have started, or a
      // request put on screen in its place while the answer was out stays,
      // and the returned request waits in the queue, marked as not answered,
      // with the status line saying why. Selecting arms nothing; deciding it
      // again still takes two presses.
      return state.selected === null &&
        state.intent === null &&
        (state.presented === null || state.presented === action.id)
        ? { selected: action.id, intent: null, presented: action.id }
        : state;
    case "queue": {
      const present = new Set(action.ids);
      const selected =
        state.selected !== null && present.has(state.selected) ? state.selected : null;
      const intent = state.intent !== null && present.has(state.intent.id) ? state.intent : null;
      // The request on screen stays while it is waiting; only once it has
      // left does another take its place, and that one is then held in turn.
      const presented = shownId({ selected, intent, presented: state.presented }, action.ids);
      return selected === state.selected &&
        intent === state.intent &&
        presented === state.presented
        ? state
        : { selected, intent, presented };
    }
  }
}
