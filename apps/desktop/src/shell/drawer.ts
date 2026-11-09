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
 */

import { type RefObject, useEffect, useRef } from "react";

import { focusables, trapTab } from "./modal";

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
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      trapTab(container, event);
    };
    container.addEventListener("keydown", onKeyDown);

    return () => {
      container.removeEventListener("keydown", onKeyDown);
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
