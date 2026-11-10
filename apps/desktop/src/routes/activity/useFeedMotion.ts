import { type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { EventView } from "../../bindings/EventView";
import { countAfter, freshArrivals } from "./feedSignals";

/**
 * The ids of events that arrived since the feed was last drawn, so their rows
 * can rise into place.
 *
 * The window as it stood at the last commit is kept in a ref and replaced
 * after each commit. The arrivals are worked out once per change to the
 * window, not once per render, so the class a row gets stays until the window
 * next changes: a render for any other reason (a minute ticking over, a copy
 * confirming) would otherwise take it away halfway through the entrance and
 * snap the row into place. `source` names where the window comes from (the
 * stream, or the security view): when it changes the window has been
 * replaced, not added to, and nothing counts as arrived.
 */
export function useArrivals(events: readonly EventView[], source: string): ReadonlySet<string> {
  const known = useRef<ReadonlySet<string> | null>(null);
  const knownSource = useRef(source);
  const fresh = useMemo(
    () => (knownSource.current === source ? freshArrivals(known.current, events) : EMPTY),
    [events, source],
  );

  useEffect(() => {
    knownSource.current = source;
    // Not primed until the window holds something: the first read of history
    // is the feed arriving, not news arriving in it.
    known.current = events.length === 0 ? null : new Set(events.map((event) => event.id));
  }, [events, source]);

  return fresh;
}

const EMPTY: ReadonlySet<string> = new Set();

/**
 * How many events have come in since Follow was switched off, for the button
 * that jumps back to the newest.
 *
 * The anchor is the newest event on screen at the moment Follow went off, or
 * when the feed first held something if Follow was already off.
 */
export function useUnseen(follow: boolean, events: readonly EventView[]): number {
  const newest = useRef<string | null>(null);
  newest.current = events.at(-1)?.id ?? null;
  const [anchor, setAnchor] = useState<string | null>(null);
  const holding = events.length > 0;

  useEffect(() => {
    setAnchor((current) => (follow ? null : (current ?? newest.current)));
  }, [follow, holding]);

  return follow ? 0 : countAfter(events, anchor);
}

/**
 * Tell the screen's sticky parts how far down the pinned page header reaches,
 * as `--activity-pin` on `root`, so the day separators and the open record
 * pin beneath the header rather than under it.
 *
 * The header's height is not fixed: its actions can wrap, and a coarse pointer
 * makes every control taller. It is measured once before paint and again
 * whenever it resizes. The narrow layout does not pin the header, and its
 * stylesheet pins beneath the top bar instead.
 */
export function usePinOffset(root: RefObject<HTMLElement | null>): void {
  useLayoutEffect(() => {
    const host = root.current;
    const bar = host?.querySelector<HTMLElement>(":scope > .page-head.sticky") ?? null;
    if (!host || !bar) return;
    const apply = () => {
      const top = Number.parseFloat(getComputedStyle(bar).top) || 0;
      host.style.setProperty("--activity-pin", `${Math.round(top + bar.offsetHeight)}px`);
    };
    apply();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(apply);
    observer.observe(bar);
    return () => observer.disconnect();
  }, [root]);
}
