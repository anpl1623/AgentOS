import type { ReactNode } from "react";

/**
 * A key, or a short chord written as one, such as `⌘K`.
 *
 * Inside a control whose shortcut is announced some other way — the control's
 * `aria-keyshortcuts`, or the shortcut sheet — pass `hidden`, so the key is
 * drawn for the eye and not read twice.
 */
export function Kbd({ children, hidden = false }: { children: ReactNode; hidden?: boolean | undefined }) {
  return (
    <kbd className="kbd" aria-hidden={hidden ? true : undefined}>
      {children}
    </kbd>
  );
}
