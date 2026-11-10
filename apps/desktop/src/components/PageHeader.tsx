import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";

import { Icon } from "./icons";

/**
 * The title block of a screen.
 *
 * Top to bottom: a breadcrumb back to the list a detail screen belongs to, a
 * small eyebrow over the title, the title with the screen's actions beside it,
 * and a sentence under it. Only the title row is sticky. Once the main column
 * scrolls past it, the row pins to the top as a glass bar with a hairline, so
 * the screen's name and its actions stay in reach on a long page; the
 * breadcrumb, eyebrow and sentence scroll away, because they orient a person
 * arriving and are not needed while reading. In the narrow layout the top bar
 * already names the screen, so the row does not pin there.
 *
 * `leading` is drawn before the title and outside the heading's text — an
 * agent's avatar, say — so the heading names the screen in words alone.
 *
 * `clampPinned` keeps a long title to one line while the row is pinned. The
 * height the clamp takes from the bar is handed to the bar's margin, so the
 * row holds the same place in the column pinned or not and nothing below it
 * moves.
 *
 * In the narrow layout the actions wrap beneath the title, and would come
 * between it and the sentence that explains it. The sentence is therefore
 * also drawn inside the row, after the title, where the narrow layout shows it
 * in place of the one below the row; the stylesheet shows exactly one of the
 * two, and the other is `display: none`, so it is neither seen nor read twice.
 *
 * `parent` takes a callback rather than a route so this component does not
 * need to know how the app navigates.
 */
export function PageHeader({
  eyebrow,
  title,
  subtitle,
  parent,
  actions,
  leading,
  sticky = true,
  clampPinned = false,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  parent?: { label: string; onActivate: () => void } | undefined;
  actions?: ReactNode;
  /** Drawn before the title, outside the heading. */
  leading?: ReactNode;
  /** Pin the title row while the screen scrolls; on by default. */
  sticky?: boolean | undefined;
  /** Keep a pinned title to one line, without moving anything beneath it. */
  clampPinned?: boolean | undefined;
}) {
  const sentinel = useRef<HTMLDivElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  const stuck = useStuck(sticky, sentinel, bar);
  useClampHold(clampPinned && stuck, bar);

  const headClass = ["page-head", sticky ? "sticky" : null, stuck ? "stuck" : null]
    .filter(Boolean)
    .join(" ");
  const heading = <h1 className="page-title">{title}</h1>;

  return (
    <>
      {parent ? (
        <nav className="crumbs" aria-label="Breadcrumb">
          <button type="button" className="crumb" onClick={parent.onActivate}>
            <Icon name="chevron-left" size={14} />
            {parent.label}
          </button>
        </nav>
      ) : null}
      {eyebrow ? <div className="page-eyebrow">{eyebrow}</div> : null}
      {/* Zero height, just above the row: when it leaves the top of the
          column, the row has pinned. */}
      {sticky ? <div ref={sentinel} className="page-sentinel" aria-hidden="true" /> : null}
      <div ref={bar} className={headClass}>
        {leading ? (
          <div className="page-title-lead">
            {leading}
            {heading}
          </div>
        ) : (
          heading
        )}
        {subtitle && actions ? <p className="page-sub page-sub-inline">{subtitle}</p> : null}
        {actions ? <div className="page-actions">{actions}</div> : null}
      </div>
      {subtitle ? <p className="page-sub">{subtitle}</p> : null}
    </>
  );
}

/**
 * Whether the sticky row has pinned, watched with an intersection observer on
 * a marker just above it rather than a scroll listener, so scrolling a long
 * screen costs nothing until the moment it crosses.
 *
 * The observer's root is the viewport, which a column scrolled out of view
 * clips its content against, so it works whatever element scrolls. The row's
 * own `top` is read from the stylesheet so the two agree on where pinned is.
 */
function useStuck(
  enabled: boolean,
  sentinel: RefObject<HTMLDivElement | null>,
  bar: RefObject<HTMLDivElement | null>,
): boolean {
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const marker = sentinel.current;
    const row = bar.current;
    if (!enabled || marker === null || row === null) return;
    if (typeof IntersectionObserver === "undefined") return;
    const offset = Math.max(0, Number.parseFloat(getComputedStyle(row).top) || 0);
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry === undefined) return;
        const top = entry.rootBounds?.top ?? offset;
        setStuck(!entry.isIntersecting && entry.boundingClientRect.top < top);
      },
      { rootMargin: `-${offset + 1}px 0px 0px 0px`, threshold: 0 },
    );
    observer.observe(marker);
    return () => observer.disconnect();
  }, [enabled, sentinel, bar]);
  return enabled && stuck;
}

/**
 * Clamp a pinned title to one line and give back the height it took away.
 *
 * Measured before paint: the row's height as drawn, then with the clamp, and
 * the difference is written as `--page-head-hold`, which the stylesheet adds
 * to the row's bottom margin. The row is a sticky box, so its margin is what
 * holds its place in the column; the content beneath therefore stays exactly
 * where it was. Measured again when the window is resized, since a resize
 * can change how many lines the title would take; and the stylesheet ignores
 * the clamp in the narrow layout, where the measurement then comes out as
 * nothing.
 *
 * The attribute and the property are set on the element directly rather
 * than through React, because the measurement needs the row drawn both ways
 * within one layout pass.
 */
function useClampHold(active: boolean, bar: RefObject<HTMLDivElement | null>): void {
  useLayoutEffect(() => {
    const row = bar.current;
    if (!active || row === null) return;
    const measure = () => {
      row.removeAttribute("data-clamped");
      row.style.removeProperty("--page-head-hold");
      const full = row.getBoundingClientRect().height;
      row.setAttribute("data-clamped", "");
      const clamped = row.getBoundingClientRect().height;
      row.style.setProperty("--page-head-hold", `${Math.max(0, full - clamped)}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("resize", measure);
      row.removeAttribute("data-clamped");
      row.style.removeProperty("--page-head-hold");
    };
  }, [active, bar]);
}
