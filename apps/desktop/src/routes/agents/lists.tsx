import type { ReactNode } from "react";

/**
 * A list, named by its heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`, as on the dashboard: the
 * stylesheets have no reset for a bare list. What a screen reader hears, the
 * name and the count, is the same.
 */
export function List({
  labelledBy,
  label,
  className,
  children,
}: {
  labelledBy?: string | undefined;
  label?: string | undefined;
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div role="list" aria-labelledby={labelledBy} aria-label={label} className={className}>
      {children}
    </div>
  );
}

/** One entry of a {@link List}. A `Row` that is a button cannot also be the item. */
export function Item({ children }: { children: ReactNode }) {
  return <div role="listitem">{children}</div>;
}
