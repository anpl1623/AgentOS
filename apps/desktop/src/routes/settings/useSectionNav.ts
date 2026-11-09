/**
 * The in-page navigation's two hooks: which section is being read, and where
 * the marker behind the current entry sits.
 */

import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { prefersReducedMotion } from "../../components/motion";
import { activeSection, stripScroll } from "./sectionsModel";

/**
 * How long a jump keeps its section marked while the scroll it started runs.
 * A smooth scroll passes every section between here and there, and marking
 * each on the way would flicker the navigation through them.
 */
const JUMP_HOLD_MS = 900;

/**
 * How far below the place a jump lands a heading the reading line sits. Just
 * enough that the heading jumped to has reached it and the next has not,
 * however short the section between them.
 */
const LINE_SLACK = 24;

/** Room left either side of a chip scrolled into view in the strip. */
const STRIP_PAD = 24;

/** The nearest ancestor that scrolls, or `null` when the document itself does. */
function scrollParent(element: HTMLElement): HTMLElement | null {
  for (let node = element.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY === "auto" || overflowY === "scroll") return node;
  }
  return null;
}

/** The section a heading opens, which is what is measured and scrolled to. */
function sectionOf(heading: HTMLElement): HTMLElement {
  return heading.closest<HTMLElement>("section") ?? heading;
}

function pixels(value: string): number {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * Which of the sections whose headings carry `ids` is being read, and a way to
 * jump to one.
 *
 * Measured on scroll, at most once a frame: nine headings' positions are
 * cheap, and an observer of a thin band would still need the foot of the page
 * handled apart. The reading line is worked out from the same scroll padding
 * and margin a jump lands by, so whichever layout is showing, the section
 * jumped to is the section marked.
 */
export function useSectionSpy(ids: readonly string[]): {
  active: string | null;
  jump: (id: string) => void;
} {
  const [active, setActive] = useState<string | null>(ids[0] ?? null);
  const holdUntil = useRef(0);
  const key = ids.join(" ");

  useEffect(() => {
    const list = key.split(" ");
    const first = document.getElementById(list[0] ?? "");
    if (first === null) return;
    const scroller = scrollParent(first);
    const view = scroller ?? document.documentElement;
    let frame = 0;

    const measure = () => {
      frame = 0;
      if (performance.now() < holdUntil.current) return;
      const origin = scroller ? scroller.getBoundingClientRect().top : 0;
      const padding = scroller ? pixels(getComputedStyle(scroller).scrollPaddingTop) : 0;
      const margin = pixels(getComputedStyle(sectionOf(first)).scrollMarginTop);
      const tops = list.flatMap((id) => {
        const heading = document.getElementById(id);
        return heading ? [{ id, top: sectionOf(heading).getBoundingClientRect().top }] : [];
      });
      const room = view.scrollHeight - view.clientHeight;
      const atEnd = room > 1 && view.scrollTop >= room - 2;
      const next = activeSection(tops, origin + padding + margin + LINE_SLACK, atEnd);
      setActive((previous) => (previous === next ? previous : next));
    };
    const schedule = () => {
      if (frame === 0) frame = window.requestAnimationFrame(measure);
    };

    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      target.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      if (frame !== 0) window.cancelAnimationFrame(frame);
    };
  }, [key]);

  const jump = useCallback((id: string) => {
    const heading = document.getElementById(id);
    if (heading === null) return;
    holdUntil.current = performance.now() + JUMP_HOLD_MS;
    setActive(id);
    sectionOf(heading).scrollIntoView({
      behavior: prefersReducedMotion() ? "auto" : "smooth",
      block: "start",
    });
    // As a link to a fragment would: the next Tab starts in the section.
    heading.focus({ preventScroll: true });
  }, []);

  return { active, jump };
}

/** Where the marker sits within the navigation's track. */
export interface MarkerBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

function sameBox(a: MarkerBox | null, b: MarkerBox | null): boolean {
  return (
    a !== null &&
    b !== null &&
    a.left === b.left &&
    a.top === b.top &&
    a.width === b.width &&
    a.height === b.height
  );
}

/**
 * The marker behind the current entry: its box, measured before paint and
 * again whenever the track is resized — the layout changing from a column to
 * a strip, text being enlarged — and whether it may slide yet. As in the
 * sidebar, it does not slide on the first paint, since there is nowhere it
 * came from.
 *
 * In the strip, the current chip is also scrolled into view, by no more than
 * it takes, so marking a section never leaves its chip off the edge.
 */
export function useMarker(
  track: RefObject<HTMLElement | null>,
  active: string | null,
): { box: MarkerBox | null; ready: boolean } {
  const [box, setBox] = useState<MarkerBox | null>(null);
  const [ready, setReady] = useState(false);
  const readyRef = useRef(false);

  useLayoutEffect(() => {
    const element = track.current;
    if (element === null) return;
    const measure = () => {
      const item = element.querySelector<HTMLElement>('[aria-current="location"]');
      const next =
        item === null
          ? null
          : {
              left: item.offsetLeft,
              top: item.offsetTop,
              width: item.offsetWidth,
              height: item.offsetHeight,
            };
      setBox((previous) => (sameBox(previous, next) ? previous : next));
      if (item !== null && element.scrollWidth > element.clientWidth) {
        const left = stripScroll(
          item.offsetLeft,
          item.offsetWidth,
          element.scrollLeft,
          element.clientWidth,
          STRIP_PAD,
        );
        if (left !== element.scrollLeft) {
          element.scrollTo({
            left,
            behavior: readyRef.current && !prefersReducedMotion() ? "smooth" : "auto",
          });
        }
      }
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [track, active]);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      readyRef.current = true;
      setReady(true);
    });
    return () => window.cancelAnimationFrame(frame);
  }, []);

  return { box, ready };
}
