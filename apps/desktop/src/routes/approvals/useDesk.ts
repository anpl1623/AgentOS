import {
  type Dispatch,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { SCROLL_REGION } from "../../components/ArgumentBlock";
import { isTypingTarget } from "../../shell/keys";
import { modalOpen } from "../../shell/modal";
import {
  type Selection,
  type SelectionAction,
  arrivals,
  idleSelection,
  nextSelection,
} from "./approvalsModel";
import { shownId } from "./deskModel";

/** The selection keys. Movement only; see the keyboard effect for why. */
const MOVES: Readonly<Record<string, 1 | -1>> = { j: 1, ArrowDown: 1, k: -1, ArrowUp: -1 };

/**
 * Marks the side-by-side queue. A move made from inside it keeps the keyboard
 * in it, so the queue reads like a list; a move from anywhere else puts the
 * keyboard on the card.
 */
export const QUEUE_RAIL = "[data-queue-rail]";

/** A card to bring into view after the next render, and where the keyboard goes. */
interface Reveal {
  id: string;
  focus: "rail" | "card" | null;
}

export interface Desk {
  /** The waiting cards' ids, in reading order. */
  ids: readonly string[];
  selection: Selection;
  dispatch: Dispatch<SelectionAction>;
  /** The request presented as the one to read next; see `shownId`. */
  shown: string | null;
  /** Where a card stood in the queue, for a departure; -1 if it is not waiting. */
  indexOf: (id: string) => number;
  register: (id: string, element: HTMLElement | null) => void;
  registerItem: (id: string, element: HTMLElement | null) => void;
  /** A link named a request that is no longer waiting. */
  focusMissing: boolean;
  /** The last arrival notice, for the screen's polite region. */
  arrival: string;
}

/**
 * The queue's selection, its keyboard, its deep link and its arrival notices.
 *
 * A card can be hidden when it is selected: side by side, only the presented
 * card is drawn in full, and the stylesheet decides that. So focus and scroll
 * never happen in the same breath as the selection change; they are asked for
 * here and carried out after the render that shows the card.
 */
export function useDesk(
  sorted: readonly ApprovalView[],
  focus: string | undefined,
  loaded: boolean,
): Desk {
  const ids = useMemo(() => sorted.map((approval) => approval.id), [sorted]);
  const [selection, dispatch] = useReducer(nextSelection, idleSelection);
  const [reveal, setReveal] = useState<Reveal | null>(null);
  const [focusMissing, setFocusMissing] = useState(false);
  const cards = useRef(new Map<string, HTMLElement>());
  const items = useRef(new Map<string, HTMLElement>());
  const order = useRef(ids);
  order.current = ids;
  const current = useRef(selection);
  current.current = selection;

  // A card that leaves the queue takes its selection and its intent with it.
  useEffect(() => dispatch({ kind: "queue", ids }), [ids]);

  // New requests are announced in one line each time the queue grows; what
  // is in the queue when it first loads is not announced at all.
  const seen = useRef<ReadonlySet<string> | null>(null);
  const [arrival, setArrival] = useState("");
  useEffect(() => {
    if (!loaded) return;
    const next = arrivals(seen.current, sorted);
    seen.current = next.seen;
    if (next.notice !== null) setArrival(next.notice);
  }, [loaded, sorted]);

  // A link names one card: select it and bring it into view once, when the
  // queue has loaded. Focus stays where the shell puts it on arrival.
  const landed = useRef<string | null>(null);
  useEffect(() => {
    if (focus === undefined || !loaded || landed.current === focus) return;
    landed.current = focus;
    const present = cards.current.has(focus);
    setFocusMissing(!present);
    if (!present) return;
    dispatch({ kind: "select", id: focus });
    setReveal({ id: focus, focus: null });
  }, [focus, loaded, ids]);

  // Carried out after the render that drew the card. A queue item hidden by
  // the stacked layout cannot take focus, so the card takes it instead.
  useEffect(() => {
    if (reveal === null) return;
    const card = cards.current.get(reveal.id);
    const item = items.current.get(reveal.id);
    if (reveal.focus === "rail") item?.focus({ preventScroll: true });
    if (reveal.focus === "card" || (reveal.focus === "rail" && document.activeElement !== item)) {
      card?.focus({ preventScroll: true });
    }
    item?.scrollIntoView({ block: "nearest" });
    card?.scrollIntoView({ block: "nearest" });
  }, [reveal]);

  // The keyboard moves through the queue and can set a half-made decision
  // down. It cannot make one. There is deliberately no key for approve, deny
  // or confirm: the queue reflows the moment a decision is confirmed, so a
  // repeated or queued keystroke lands on whichever card slid into the
  // decided one's place, and that is how an operator approves a request they
  // never read. Approve and Deny are ordinary buttons, reached with Tab and
  // pressed as buttons are, and each still takes a second, confirming press.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || modalOpen()) return;
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.key === "Escape") {
        // Honoured from the note field too: it is the way out of the field.
        if (current.current.intent === null) return;
        event.preventDefault();
        dispatch({ kind: "cancel" });
        return;
      }
      if (isTypingTarget(event.target)) return;
      const by = MOVES[event.key];
      if (by === undefined) return;
      const arrow = event.key === "ArrowDown" || event.key === "ArrowUp";
      const within = (selector: string) =>
        event.target instanceof Element && event.target.closest(selector) !== null;
      if (arrow && within(SCROLL_REGION)) return;
      event.preventDefault();
      // A move puts the keyboard on the card it selected, so Tab continues
      // into that card rather than from wherever focus last was; or, from the
      // side-by-side queue, on that card's place in the queue.
      const move = { kind: "move", by, order: order.current } as const;
      const target = nextSelection(current.current, move).selected;
      dispatch(move);
      if (target !== null) setReveal({ id: target, focus: within(QUEUE_RAIL) ? "rail" : "card" });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const register = useCallback((id: string, element: HTMLElement | null) => {
    if (element === null) cards.current.delete(id);
    else cards.current.set(id, element);
  }, []);
  const registerItem = useCallback((id: string, element: HTMLElement | null) => {
    if (element === null) items.current.delete(id);
    else items.current.set(id, element);
  }, []);
  const indexOf = useCallback((id: string) => order.current.indexOf(id), []);

  return {
    ids,
    selection,
    dispatch,
    shown: shownId(selection, ids),
    indexOf,
    register,
    registerItem,
    focusMissing,
    arrival,
  };
}
