/**
 * What a decision says, and the cards leaving the queue, without React.
 *
 * The sentences are the page's status line and its receipts: the record of
 * what the operator did, said the moment they did it. The departures are only
 * a picture of the same thing — a card stamped with its answer and folding
 * away — and are kept apart from the record on purpose: the store has already
 * taken the card out of the queue and the status line has already spoken, so
 * nothing about the outcome waits on, or is hidden by, the animation.
 */

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { IntentKind } from "./approvalsModel";

/** What became of a run the operator asked to stop. */
export type RunEnd = { kind: "stopped" } | { kind: "ended" } | { kind: "failed"; message: string };

/** A decision made while this screen was open, for the receipts above the queue. */
export interface Receipt {
  id: string;
  kind: IntentKind;
  tool: string;
  agent: string;
  at: string;
  /** For a denial that also stopped the run: what became of the run. */
  run?: RunEnd["kind"];
}

export const DONE: Readonly<Record<IntentKind, string>> = {
  approve: "Approved",
  deny: "Denied",
  stop: "Denied",
};

const DOING: Readonly<Record<IntentKind, string>> = {
  approve: "Approving",
  deny: "Denying",
  stop: "Denying",
};

const RUN_SENTENCE: Readonly<Record<RunEnd["kind"], string>> = {
  stopped: "The run was stopped.",
  ended: "The run had already ended.",
  failed: "The run could not be stopped:",
};

/** How a receipt ends for a denial that also asked the run to stop. */
export const RUN_CLAUSE: Readonly<Record<RunEnd["kind"], string>> = {
  stopped: " and stopped the run",
  ended: " (the run had already ended)",
  failed: " (the run could not be stopped)",
};

function what(approval: Pick<ApprovalView, "tool" | "agent_name">): string {
  return `${approval.tool} for ${approval.agent_name}`;
}

/** The status line while the answer is on its way. */
export function pendingLine(
  kind: IntentKind,
  approval: Pick<ApprovalView, "tool" | "agent_name">,
): string {
  return `${DOING[kind]} ${what(approval)}${kind === "stop" ? " and stopping the run" : ""}…`;
}

/** The status line once the answer was delivered, or found nothing waiting (`movedOn`). */
export function settledLine(
  kind: IntentKind,
  approval: Pick<ApprovalView, "tool" | "agent_name">,
  movedOn: string | null,
  run: RunEnd | undefined,
): string {
  const said = movedOn ?? `${DONE[kind]} ${what(approval)}.`;
  if (run === undefined) return said;
  return `${said} ${RUN_SENTENCE[run.kind]}${run.kind === "failed" ? ` ${run.message}` : ""}`;
}

/** The status line when the answer could not be sent. */
export function failedLine(message: string): string {
  return `Could not answer: ${message} The request is back in the list; nothing was changed.`;
}

/**
 * How the status line is drawn, beside what it says: an answer on its way, one
 * that was given, one given while part of what was asked did not happen, or
 * one that could not be sent. The words carry all of it; the hue on the
 * line's leading edge only repeats them.
 */
export type StatusTone = "working" | "done" | "partial" | "failed";

/**
 * The tone once an answer was delivered or found nothing waiting. A run that
 * was asked to stop and could not be is the part that did not happen.
 */
export function settledTone(run: RunEnd | undefined): StatusTone {
  return run?.kind === "failed" ? "partial" : "done";
}

/** The word stamped on a departing card. */
export const STAMP: Readonly<Record<IntentKind, string>> = {
  approve: "Approved",
  deny: "Denied",
  stop: "Denied, stopping the run",
};

// ---------------------------------------------------------------------------
// Departures
// ---------------------------------------------------------------------------

/**
 * A card on its way out: first stamped with the answer, then folding shut so
 * the cards below it rise into its place rather than jumping.
 */
export interface Departure {
  approval: ApprovalView;
  kind: IntentKind;
  /** Where it stood in the queue when it was answered. */
  index: number;
  phase: "stamp" | "collapse";
}

/** Start a departure; a second answer to the same card replaces the first. */
export function depart(
  list: readonly Departure[],
  approval: ApprovalView,
  kind: IntentKind,
  index: number,
): Departure[] {
  return [
    ...list.filter((each) => each.approval.id !== approval.id),
    { approval, kind, index: Math.max(0, index), phase: "stamp" },
  ];
}

/** The stamp has been seen; fold the card. */
export function collapse(list: readonly Departure[], id: string): Departure[] {
  return list.map((each) =>
    each.approval.id === id && each.phase === "stamp" ? { ...each, phase: "collapse" } : each,
  );
}

/** The card has gone, or came back to the queue. */
export function settle(list: readonly Departure[], id: string): Departure[] {
  return list.filter((each) => each.approval.id !== id);
}

/** One place in the drawn queue: a waiting card, or one leaving. */
export type DeskEntry =
  { kind: "card"; approval: ApprovalView; index: number } | { kind: "ghost"; departure: Departure };

/**
 * The queue as drawn: the waiting cards in reading order, with each departing
 * card back at the place it left, so the cards after it close up only as it
 * folds. A departing card whose request is waiting again — its answer failed
 * and the store restored it — is dropped: the real card is back, and the two
 * must never be on screen together.
 */
export function deskEntries(
  cards: readonly ApprovalView[],
  departures: readonly Departure[],
): DeskEntry[] {
  const waiting = new Set(cards.map((card) => card.id));
  const entries: DeskEntry[] = cards.map((approval, index) => ({ kind: "card", approval, index }));
  // A place is counted among the cards still waiting when the answer was
  // given, so a card answered just after the one above it left records the
  // same place. Placed lowest first and, within a place, in the order they
  // were answered (the sort is stable), each one after every departing card
  // already placed at or above it, so two cards answered in a row keep their
  // order.
  const leaving = departures
    .filter((each) => !waiting.has(each.approval.id))
    .sort((a, b) => a.index - b.index);
  leaving.forEach((departure, placed) => {
    entries.splice(Math.min(departure.index + placed, entries.length), 0, {
      kind: "ghost",
      departure,
    });
  });
  return entries;
}
