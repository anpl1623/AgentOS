/**
 * The agents screen's hooks: when the data on screen was read, which section
 * of the detail page is being read, and how the page moves to one.
 *
 * The decisions are pure functions in `detailModel`; these only measure and
 * schedule.
 */

import { useEffect, useState } from "react";

import { prefersReducedMotion } from "../../components/motion";
import { type DetailSection, READING_LINE, activeSection, scrolledToEnd } from "./detailModel";

/**
 * When the data on screen was read, for the stale marker. Set on each new
 * answer rather than each render, so a failed refresh leaves it where it was.
 */
export function useLoadedAt(data: unknown): number {
  const [loadedAt, setLoadedAt] = useState(() => Date.now());
  useEffect(() => {
    if (data !== null) setLoadedAt(Date.now());
  }, [data]);
  return loadedAt;
}

/**
 * The id of the section being read, for the in-page navigation.
 *
 * Measured at most once a frame while anything scrolls, and on resize. Scroll
 * events do not bubble, so the listener is on the document in the capture
 * phase, which every scrolling element's event passes through on its way down;
 * the main column is what scrolls, but nothing here needs to know that. Not
 * measured until `ready`, because the headings are not drawn until the agent
 * has loaded.
 */
export function useActiveSection(
  sections: readonly DetailSection[],
  ready: boolean,
): string | null {
  const [active, setActive] = useState<string | null>(sections[0]?.id ?? null);

  useEffect(() => {
    if (!ready) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      const positions = sections.flatMap(({ id }) => {
        const heading = document.getElementById(id);
        return heading ? [{ id, top: heading.getBoundingClientRect().top }] : [];
      });
      const column = document.getElementById("main");
      const atEnd =
        column !== null &&
        column.scrollHeight > column.clientHeight &&
        scrolledToEnd(column.scrollTop, column.clientHeight, column.scrollHeight);
      setActive(activeSection(positions, READING_LINE, atEnd));
    };
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(measure);
    };
    measure();
    document.addEventListener("scroll", schedule, { capture: true, passive: true });
    window.addEventListener("resize", schedule);
    return () => {
      document.removeEventListener("scroll", schedule, { capture: true });
      window.removeEventListener("resize", schedule);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  }, [sections, ready]);

  return active;
}

/**
 * Bring a section's heading into view and give it focus, so the next Tab
 * starts inside that section rather than back at the navigation.
 *
 * The heading's own `scroll-margin-top` keeps it clear of the pinned page
 * header. Smooth only when the person has not asked for less motion.
 */
export function jumpToSection(id: string): void {
  const heading = document.getElementById(id);
  if (heading === null) return;
  heading.scrollIntoView({
    behavior: prefersReducedMotion() ? "auto" : "smooth",
    block: "start",
  });
  heading.focus({ preventScroll: true });
}
