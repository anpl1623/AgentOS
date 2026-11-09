import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";

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
 * `parent` takes a callback rather than a route so this component does not
 * need to know how the app navigates.
 */
export function PageHeader({
  eyebrow,
  title,
  subtitle,
  parent,
  actions,
  sticky = true,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  parent?: { label: string; onActivate: () => void } | undefined;
  actions?: ReactNode;
  /** Pin the title row while the screen scrolls; on by default. */
  sticky?: boolean | undefined;
}) {
  const sentinel = useRef<HTMLDivElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  const stuck = useStuck(sticky, sentinel, bar);

  const headClass = ["page-head", sticky ? "sticky" : null, stuck ? "stuck" : null]
    .filter(Boolean)
    .join(" ");

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
        <h1 className="page-title">{title}</h1>
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
