import type { ReactNode } from "react";

import { Icon, type IconName } from "../../components/common";
import { section } from "./sectionsModel";

/**
 * A section of the screen: its heading, a sentence on what it governs, and
 * the section itself. The heading's id names the lists beneath it and is
 * where the in-page navigation jumps to; it takes focus after a jump
 * (`tabIndex={-1}`), so the next Tab starts inside the section rather than
 * back at the navigation.
 *
 * The name and icon come from the section list, so the heading and its entry
 * in the navigation cannot drift apart.
 */
export function Section({
  id,
  intro,
  actions,
  children,
}: {
  id: string;
  intro?: ReactNode;
  /** Controls that act on the whole section, drawn at the end of the heading row. */
  actions?: ReactNode;
  children: ReactNode;
}) {
  const entry = section(id);
  return (
    <section className="settings-section" aria-labelledby={id}>
      <div className="settings-section-head">
        <span className="settings-section-mark" aria-hidden="true">
          <Icon name={entry.icon} size={16} />
        </span>
        <h2 id={id} tabIndex={-1}>
          {entry.label}
        </h2>
        {actions ? <div className="settings-section-actions">{actions}</div> : null}
      </div>
      {intro ? <p className="settings-section-intro">{intro}</p> : null}
      {children}
    </section>
  );
}

/**
 * A list of rows, named by its heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`, as on the dashboard: the
 * stylesheets have no reset for a bare list. What a screen reader hears is the
 * same.
 */
export function List({
  labelledBy,
  className,
  children,
}: {
  labelledBy: string;
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div role="list" aria-labelledby={labelledBy} className={className}>
      {children}
    </div>
  );
}

/** One entry of a {@link List}. */
export function Item({ children }: { children: ReactNode }) {
  return <div role="listitem">{children}</div>;
}

/**
 * A chip saying whether something is in place.
 *
 * The verdict shape, since a stored key or a verified chain is a fact found,
 * not a risk. The hidden noun says what the chip is about, as every chip in
 * `status.tsx` does.
 */
export function Fact({ tone, noun, children }: { tone: string; noun: string; children: string }) {
  return (
    <span className={`verdict ${tone}`}>
      <span className="visually-hidden">{noun} </span>
      {children}
    </span>
  );
}

/**
 * The small tile at the head of a row: what kind of thing the row is, at a
 * glance down the list. Decoration; the row's own words say the same.
 */
export function RowMark({
  icon,
  tone,
}: {
  icon: IconName;
  tone?: "ok" | "danger" | "neutral" | undefined;
}) {
  return (
    <span className={`settings-row-mark tone-${tone ?? "neutral"}`} aria-hidden="true">
      <Icon name={icon} size={16} />
    </span>
  );
}
