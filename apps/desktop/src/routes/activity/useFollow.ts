import { useEffect, useRef } from "react";

import { setFeedFilter } from "../../sdk/eventStream";
import { USER_SCROLL_WINDOW_MS, followAfterScroll } from "./activityModel";

/**
 * Keep the newest event in view while Follow is on, and let the operator's
 * own scrolling switch it.
 *
 * Follow fought the operator when it was only a toggle: it kept pulling the
 * page to the bottom while they read. Now a scroll they make more than the
 * threshold away from the end turns it off and a scroll back turns it on.
 * Scrolls the screen makes itself — the jump to the newest event, the shell
 * restoring a remembered position — are told apart by whether a wheel, touch,
 * key or held pointer just happened, and change nothing.
 *
 * Returns the marker at the end of the feed.
 */
export function useFollow(follow: boolean, newest: string | undefined) {
  const end = useRef<HTMLDivElement>(null);
  const followRef = useRef(follow);
  followRef.current = follow;

  useEffect(() => {
    const marker = end.current;
    if (marker === null) return;
    const scroller = scrollContainer(marker);
    const target: HTMLElement | Window = scroller ?? window;
    let lastInput = Number.NEGATIVE_INFINITY;
    let pointerDown = false;

    const input = () => {
      lastInput = performance.now();
    };
    const down = () => {
      pointerDown = true;
      input();
    };
    const up = () => {
      pointerDown = false;
    };
    const scroll = () => {
      const byOperator = pointerDown || performance.now() - lastInput < USER_SCROLL_WINDOW_MS;
      const bottom = scroller ? scroller.getBoundingClientRect().bottom : window.innerHeight;
      const distance = marker.getBoundingClientRect().bottom - bottom;
      const next = followAfterScroll(followRef.current, distance, byOperator);
      if (next !== followRef.current) setFeedFilter({ follow: next });
    };

    const inputs = ["wheel", "touchmove", "keydown"] as const;
    for (const kind of inputs) target.addEventListener(kind, input, { passive: true });
    target.addEventListener("pointerdown", down, { passive: true });
    window.addEventListener("pointerup", up, { passive: true });
    window.addEventListener("pointercancel", up, { passive: true });
    target.addEventListener("scroll", scroll, { passive: true });
    return () => {
      for (const kind of inputs) target.removeEventListener(kind, input);
      target.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      target.removeEventListener("scroll", scroll);
    };
  }, []);

  // A passive effect, so on arrival it runs after the shell has restored the
  // screen's remembered position; Follow, when on, has the last word.
  useEffect(() => {
    if (follow) end.current?.scrollIntoView({ block: "end" });
  }, [follow, newest]);

  return end;
}

/** The nearest ancestor that scrolls, or `null` when the document does. */
export function scrollContainer(element: HTMLElement): HTMLElement | null {
  for (let at = element.parentElement; at !== null; at = at.parentElement) {
    const { overflowY } = getComputedStyle(at);
    if (overflowY === "auto" || overflowY === "scroll") return at;
  }
  return null;
}
