import { type Dispatch, type RefObject, useCallback, useEffect, useRef, useState } from "react";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { prefersReducedMotion } from "../../components/motion";
import { MOVED_ON, resolveApproval } from "../../sdk/approvals";
import { api, describeError } from "../../sdk/client";
import type { Intent, IntentKind, SelectionAction } from "./approvalsModel";
import {
  type Answer,
  type Departure,
  type Receipt,
  type RunEnd,
  type StatusTone,
  answered,
  collapse,
  depart,
  failedLine,
  pendingLine,
  settle,
  settledLine,
  settledTone,
} from "./decisionModel";

/** How long a departing card shows its stamp before it folds. */
const STAMP_MS = 520;

/**
 * How long the fold takes: the stylesheet's `--dur-4`, which the fold's
 * transition reads, and a frame over, so the card is not removed mid-fold.
 */
const FOLD_MS = 440;

export interface Decisions {
  /** The page's status line: what is being done, or what was. */
  status: string;
  /** How the status line is drawn; its words say the same. */
  statusTone: StatusTone;
  statusRef: RefObject<HTMLDivElement | null>;
  receipts: readonly Receipt[];
  departures: readonly Departure[];
  decide: (approval: ApprovalView, intent: Intent) => Promise<void>;
}

/**
 * Making a decision: the status line, the receipts, and the departing card.
 *
 * The queue is the shared store's, so a decision is optimistic: the card
 * leaves and the badge drops on the confirm, before the runtime answers. What
 * was decided is then said in the page's status region, which takes focus,
 * and kept in a receipt above the queue for as long as the screen is open.
 * The departing card is a picture laid over that, never part of it: it is
 * hidden from assistive technology, takes no input, is skipped entirely under
 * reduced motion, and is withdrawn the moment an answer fails and the real
 * card comes back.
 */
export function useDecisions(
  dispatch: Dispatch<SelectionAction>,
  reloadRecent: () => void,
  indexOf: (id: string) => number,
): Decisions {
  const [status, setStatus] = useState("");
  const [statusTone, setStatusTone] = useState<StatusTone>("working");
  const [receipts, setReceipts] = useState<readonly Receipt[]>([]);
  const statusRef = useRef<HTMLDivElement>(null);
  const { departures, leave, hear, stay } = useDepartures();

  const decide = useCallback(
    async (approval: ApprovalView, intent: Intent) => {
      // Nothing stays armed and nothing stays selected: the card that takes
      // this one's place has not been read. Focus goes to the status line,
      // never to the next card's buttons, and it goes there before the card
      // leaves, so it is not dropped on the page body.
      dispatch({ kind: "resolved" });
      setStatus(pendingLine(intent.kind, approval));
      setStatusTone("working");
      statusRef.current?.focus();
      // Before the store hides the card, so the stamped copy is in its place
      // on the same frame; the two are never drawn together (`deskEntries`).
      leave(approval, intent.kind, indexOf(approval.id));

      const note = intent.note.trim();
      const outcome = await resolveApproval({
        approval_id: approval.id,
        approved: intent.kind === "approve",
        note: note === "" ? null : note,
      });
      if (outcome.kind === "failed") {
        stay(approval.id);
        setStatus(failedLine(outcome.message));
        setStatusTone("failed");
        // The store has put the request back. Present it, so the reason it
        // failed is on screen, unless another request is on screen by now:
        // that one stays, and this waits in the queue marked not answered.
        dispatch({ kind: "returned", id: approval.id });
        return;
      }
      // The stamp has said the decision is being made; now it says what the
      // runtime did with it, which for a request that had moved on is nothing.
      hear(approval.id, outcome.kind === "delivered" ? "recorded" : "moved-on");

      let run: RunEnd | undefined;
      if (intent.kind === "stop") {
        // Stopped even when the request had moved on: the operator asked for
        // the run to end, and a request that has gone says nothing about it.
        try {
          run = (await api.cancelRun(approval.run_id)) ? { kind: "stopped" } : { kind: "ended" };
        } catch (failure) {
          run = { kind: "failed", message: describeError(failure) };
        }
      }
      setStatus(
        settledLine(intent.kind, approval, outcome.kind === "delivered" ? null : MOVED_ON, run),
      );
      setStatusTone(settledTone(run));
      if (outcome.kind === "delivered") {
        setReceipts((list) => [
          {
            id: approval.id,
            kind: intent.kind,
            tool: approval.tool,
            agent: approval.agent_name,
            at: new Date().toISOString(),
            ...(run === undefined ? {} : { run: run.kind }),
          },
          ...list,
        ]);
      }
      reloadRecent();
    },
    [dispatch, reloadRecent, indexOf, leave, hear, stay],
  );

  return { status, statusTone, statusRef, receipts, departures, decide };
}

/**
 * The cards on their way out, each on two timers: stamped, then folded, then
 * gone. Nothing is scheduled under reduced motion, where a card simply leaves.
 * The stamp follows the runtime's answer (`hear`) whenever it comes; the
 * timers do not wait for it.
 */
function useDepartures(): {
  departures: readonly Departure[];
  leave: (approval: ApprovalView, kind: IntentKind, index: number) => void;
  hear: (id: string, answer: Exclude<Answer, "sent">) => void;
  stay: (id: string) => void;
} {
  const [departures, setDepartures] = useState<readonly Departure[]>([]);
  const timers = useRef(new Map<string, number[]>());

  const clear = useCallback((id: string) => {
    for (const timer of timers.current.get(id) ?? []) window.clearTimeout(timer);
    timers.current.delete(id);
  }, []);

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const list of pending.values()) for (const timer of list) window.clearTimeout(timer);
      pending.clear();
    };
  }, []);

  const leave = useCallback(
    (approval: ApprovalView, kind: IntentKind, index: number) => {
      if (prefersReducedMotion() || index < 0) return;
      const { id } = approval;
      clear(id);
      setDepartures((list) => depart(list, approval, kind, index));
      timers.current.set(id, [
        window.setTimeout(() => setDepartures((list) => collapse(list, id)), STAMP_MS),
        window.setTimeout(() => {
          timers.current.delete(id);
          setDepartures((list) => settle(list, id));
        }, STAMP_MS + FOLD_MS),
      ]);
    },
    [clear],
  );

  const hear = useCallback((id: string, answer: Exclude<Answer, "sent">) => {
    setDepartures((list) => answered(list, id, answer));
  }, []);

  const stay = useCallback(
    (id: string) => {
      clear(id);
      setDepartures((list) => settle(list, id));
    },
    [clear],
  );

  return { departures, leave, hear, stay };
}
