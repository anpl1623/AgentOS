/**
 * The navigation drawer's keyboard contract: the modal contract, for an
 * element that stays mounted and is only sometimes a modal.
 *
 * The sidebar is one element in every layout. In the compact and narrow
 * layouts it opens over the screen as a drawer, and while it is open it keeps
 * the same promises as any overlay (see `modal.ts`): focus moves in, Tab stays
 * in, Escape closes it, and focus goes back to the control that opened it. The
 * page behind it is made `inert` by the shell, so a pointer cannot reach it
 * either.
 *
 * Unlike an overlay, the drawer's keys are heard on the document rather than
 * on the drawer. A press on a part of the drawer that takes no focus — its
 * brand, its padding, and in WebKit, which does not focus a button it presses,
 * any button — can leave focus on the body, outside the drawer, where a
 * listener on the drawer would never hear Escape or Tab again. The drawer is
 * focusable itself while it is one (Sidebar.tsx), so such a press usually
 * lands on it; listening on the document keeps the promise when it does not.
 */

import { type RefObject, useEffect, useRef } from "react";

import { focusables, trapTab } from "./modal";

/** Whether a modal other than `drawer` is open, which then has the keyboard. */
function otherModalOpen(drawer: HTMLElement): boolean {
  return [...document.querySelectorAll('[aria-modal="true"]')].some((modal) => modal !== drawer);
}

/**
 * Keep the drawer contract for the element in `ref` while `open` is true.
 *
 * Focus starts on the current screen's item, so the drawer opens where the
 * operator already is and Enter takes them nowhere new.
 */
export function useDrawer(
  ref: RefObject<HTMLElement | null>,
  open: boolean,
  onClose: () => void,
): void {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const container = ref.current;
    if (!open || container === null) return;
    const opener = document.activeElement;

    const start =
      container.querySelector<HTMLElement>('[aria-current="page"]') ??
      focusables(container)[0] ??
      container;
    start.focus({ preventScroll: true });

    const onKeyDown = (event: KeyboardEvent) => {
      // A dialog raised over the drawer, such as the close guard, keeps its
      // own contract; the drawer waits beneath it.
      if (event.defaultPrevented || otherModalOpen(container)) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      // Focus outside the drawer counts as outside the cycle, so the next Tab
      // brings it back in at the near end.
      trapTab(container, event);
    };
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("keydown", onKeyDown);
      // Focus goes back to the opener only if it is still in the drawer or
      // was dropped on the body. A move to another screen has already put it
      // on that screen, and taking it back to the menu button would undo that.
      const active = document.activeElement;
      const stranded = active === null || active === document.body || container.contains(active);
      if (stranded && opener instanceof HTMLElement && opener.isConnected) {
        opener.focus({ preventScroll: true });
      }
    };
  }, [open, ref]);
}
