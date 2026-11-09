import type { ReactNode } from "react";

import { Icon, type IconName } from "../../components/common";
import { usePop } from "../../components/motion";

/**
 * A titled section of the dashboard. The heading's id is what the list inside
 * names itself by, so a screen reader moving through lists hears "Running",
 * not "list, 3 items".
 *
 * The head carries an optional count beside the title, which pops when it
 * changes, and a tail on the trailing side for the section's own control or
 * the way to the screen that has the rest. `above` sits between the head and
 * the body: an error about this section alone.
 */
export function Section({
  id,
  title,
  icon,
  count,
  tail,
  above,
  className,
  children,
}: {
  id: string;
  title: string;
  icon?: IconName | undefined;
  /** Shown beside the title once known; a change pops it. */
  count?: number | null | undefined;
  tail?: ReactNode;
  above?: ReactNode;
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div className={className ? `dash-section ${className}` : "dash-section"}>
      <div className="dash-section-head">
        {icon ? <Icon name={icon} size={15} className="dash-section-icon" /> : null}
        <h2 id={id}>{title}</h2>
        {count === null || count === undefined ? null : <Count value={count} />}
        {tail ? <div className="dash-section-tail">{tail}</div> : null}
      </div>
      {above}
      {children}
    </div>
  );
}

/** A section's count: a small pill that pops once when the number changes. */
function Count({ value }: { value: number }) {
  const { key, popping } = usePop(value);
  return (
    <span
      key={key}
      className={["dash-count", value === 0 ? "zero" : null, popping ? "pop" : null]
        .filter(Boolean)
        .join(" ")}
      aria-hidden="true"
    >
      {value}
    </span>
  );
}

/**
 * A ghost button at a section's tail that goes to the screen holding the rest:
 * the words say where, and the chevron says it leaves this one.
 *
 * Where the visible word names what to do there rather than the screen, as
 * "Verify" does, `where` finishes the name for assistive technology, so the
 * button is not heard as doing the verifying itself. It follows the visible
 * word, which stays the start of the name.
 */
export function SeeAll({
  label,
  where,
  onActivate,
}: {
  label: string;
  where?: string | undefined;
  onActivate: () => void;
}) {
  return (
    <button type="button" className="ghost dash-see-all" onClick={onActivate}>
      {label}
      {where ? <span className="visually-hidden"> {where}</span> : null}
      <Icon name="chevron-right" size={14} />
    </button>
  );
}

/**
 * A list of rows or cards, named by its section's heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`, so a list can be a grid of cards as
 * easily as a column of rows without fighting a list reset. What a screen
 * reader hears is the same.
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

/** One entry of a {@link List}. A `Row` that is a button cannot also be the item. */
export function Item({
  className,
  children,
}: {
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div role="listitem" className={className}>
      {children}
    </div>
  );
}
