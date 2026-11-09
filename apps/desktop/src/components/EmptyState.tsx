import type { ReactNode } from "react";

import { Icon, type IconName } from "./icons";

/**
 * A list or a panel with nothing in it yet, said kindly and usefully: what is
 * missing, why that is fine or what to do, and the one action that fills it.
 *
 * Announced as a status, like `Empty`, because an empty result is an answer
 * the operator asked for. `Empty` stays for a single quiet line inside a
 * panel; this is for a whole panel or screen with nothing to show.
 */
export function EmptyState({
  icon = "sparkles",
  title,
  children,
  action,
}: {
  icon?: IconName | undefined;
  title: ReactNode;
  /** A sentence or two under the title. */
  children?: ReactNode;
  /** A button or two that fill the space, such as "Create an agent". */
  action?: ReactNode;
}) {
  return (
    <div className="empty-state" role="status">
      <span className="empty-state-mark" aria-hidden="true">
        <Icon name={icon} size={22} />
      </span>
      <p className="empty-state-title">{title}</p>
      {children === undefined ? null : <div className="empty-state-body">{children}</div>}
      {action === undefined ? null : <div className="empty-state-action">{action}</div>}
    </div>
  );
}
