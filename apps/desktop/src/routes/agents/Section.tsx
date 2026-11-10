import type { ReactNode } from "react";

import { Icon, type IconName } from "../../components/common";

/**
 * One section of the agent detail page: a heading the in-page navigation can
 * move to, and what the section holds.
 *
 * The heading takes focus when the navigation moves to it (`tabIndex={-1}`:
 * focusable by script, never a Tab stop of its own), so the next Tab starts
 * inside the section. Its id is the one the section's lists are labelled by,
 * so the count beside it is kept outside it: a list is named "Memory", not
 * "Memory 2".
 */
export function Section({
  id,
  title,
  icon,
  aside,
  children,
}: {
  id: string;
  title: string;
  icon: IconName;
  /** Beside the heading: a count, or the section's own action. */
  aside?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="agent-section" aria-labelledby={id}>
      <div className="section-head">
        <span className="section-icon" aria-hidden="true">
          <Icon name={icon} size={15} />
        </span>
        <h2 id={id} className="section-title" tabIndex={-1}>
          {title}
        </h2>
        {aside === undefined ? null : <div className="section-aside">{aside}</div>}
      </div>
      {children}
    </section>
  );
}
