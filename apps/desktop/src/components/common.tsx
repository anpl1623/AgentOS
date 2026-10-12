import type { CSSProperties, ReactNode } from "react";

import { ago } from "../sdk/format";
import { useNow } from "./hooks";

// The status chips live in their own module; re-exported so every screen keeps
// importing its whole vocabulary from one place.
export * from "./status";

export function Panel({ children }: { children: ReactNode }) {
  return <div className="panel">{children}</div>;
}

/** An emptied list. A result the operator asked for, so it is announced. */
export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="empty" role="status">
      {children}
    </div>
  );
}

export function Loading({ what }: { what: string }) {
  return (
    <div className="spinner" role="status" aria-live="polite">
      Loading {what}…
    </div>
  );
}

/**
 * An async failure. Every one in the app arrives through here, so it is an
 * alert: a request that failed while the operator looked elsewhere must not
 * appear in silence.
 */
export function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="banner error" role="alert">
      {message}
    </div>
  );
}

/**
 * A labelled number for the dashboard.
 *
 * The tone is a class, not a colour. A colour chosen here is one the light
 * theme cannot reach.
 */
export function Stat({
  value,
  label,
  tone,
}: {
  value: string | number;
  label: string;
  tone?: "ok" | "warn" | "danger" | undefined;
}) {
  return (
    <div className="stat">
      <div className={tone ? `stat-value tone-${tone}` : "stat-value"}>{value}</div>
      <div className="stat-label">{label}</div>
    </div>
  );
}

/**
 * One line of a list.
 *
 * With `onActivate` it is a real `<button>`, so Enter, Space, focus and the
 * announcement as something that does a thing all come from the platform
 * rather than from a key handler that would have to be right in thirteen
 * places. Without it, it is a plain `<div>` that takes no focus.
 *
 * Pass `onActivate` only when activation does something. A row guarded with
 * `task.latest_run ? () => open(…) : undefined` is correct; a row given a
 * handler that sometimes does nothing is announced as a button, takes a tab
 * stop, and then ignores the key press. A row holding its own buttons or links
 * must not take `onActivate` either: interactive content cannot nest inside a
 * button.
 */
export function Row({
  onActivate,
  className,
  children,
}: {
  onActivate?: (() => void) | undefined;
  className?: string | undefined;
  children: ReactNode;
}) {
  const extra = className ? ` ${className}` : "";
  if (onActivate) {
    return (
      <button type="button" className={`row clickable${extra}`} onClick={onActivate}>
        {children}
      </button>
    );
  }
  return <div className={`row${extra}`}>{children}</div>;
}

/**
 * The title block of a screen.
 *
 * `parent` draws a breadcrumb back to the list a detail screen belongs to. It
 * takes a callback rather than a route so this component does not need to know
 * how the app navigates.
 */
export function PageHeader({
  title,
  subtitle,
  parent,
  actions,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  parent?: { label: string; onActivate: () => void } | undefined;
  actions?: ReactNode;
}) {
  return (
    <>
      {parent ? (
        <nav className="crumbs" aria-label="Breadcrumb">
          <button type="button" className="crumb" onClick={parent.onActivate}>
            {parent.label}
          </button>
        </nav>
      ) : null}
      <div className="page-head">
        <h1>{title}</h1>
        {actions}
      </div>
      {subtitle ? <p className="page-sub">{subtitle}</p> : null}
    </>
  );
}

// Varied so a column of placeholders reads as a list rather than a barcode.
// Fixed rather than random so a re-render does not make the skeleton jitter.
const SKELETON_WIDTHS = [
  [62, 38],
  [48, 30],
  [70, 44],
  [55, 26],
] as const;

/**
 * Placeholder rows in the shape of `.row-title` over `.row-meta`.
 *
 * Hidden from assistive technology: the `Loading` beside it is what speaks,
 * and a list of blank shapes has nothing to say.
 */
export function SkeletonRows({ count }: { count: number }) {
  return (
    <div aria-hidden="true">
      {Array.from({ length: count }, (_, index) => {
        const [title, meta] = SKELETON_WIDTHS[index % SKELETON_WIDTHS.length] ?? [60, 35];
        return (
          <div className="skeleton-row" key={index}>
            <div className="skeleton-line" style={{ "--w": `${title}%` } as CSSProperties} />
            <div className="skeleton-line" style={{ "--w": `${meta}%` } as CSSProperties} />
          </div>
        );
      })}
    </div>
  );
}

/**
 * The note on a screen whose latest refresh failed and which is showing what
 * it had before. Worded here once so every screen says it the same way.
 */
export function Stale({ since }: { since: string | number }) {
  // Re-render on the minute so "from 3m ago" does not stay 3m for an hour.
  useNow();
  const at = typeof since === "number" ? since : Date.parse(since);
  // A time that cannot be read still leaves the data stale; say so without
  // printing the garbage.
  if (!Number.isFinite(at)) return <span className="stale">showing earlier data</span>;
  return <span className="stale">showing data from {ago(new Date(at).toISOString())}</span>;
}
