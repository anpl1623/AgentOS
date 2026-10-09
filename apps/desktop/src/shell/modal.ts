/**
 * The keyboard contract every overlay in the shell keeps.
 *
 * Focus moves into the overlay when it opens, Tab cycles inside it, Escape
 * closes it, and focus returns to wherever it was when the overlay closes. A
 * modal that lets Tab wander into the page behind it leaves a keyboard user
 * typing into a screen they cannot see.
 */

import { type RefObject, useEffect, useRef } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusables(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (element) => element.getClientRects().length > 0,
  );
}

/**
 * Keep the modal contract for the element in `ref` while it is mounted.
 *
 * `initial` is focused on open; without it, the first focusable element is.
 * `onClose` is read through a ref, so an inline arrow does not re-run the
 * effect and steal focus back to the first control on every render.
 */
export function useModal(
  ref: RefObject<HTMLElement | null>,
  onClose: () => void,
  initial?: RefObject<HTMLElement | null>,
): void {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const container = ref.current;
    if (container === null) return;
    const previous = document.activeElement;

    const first = initial?.current ?? focusables(container)[0] ?? container;
    first.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const list = focusables(container);
      const head = list[0];
      const tail = list[list.length - 1];
      if (head === undefined || tail === undefined) {
        event.preventDefault();
        return;
      }
      if (event.shiftKey && document.activeElement === head) {
        event.preventDefault();
        tail.focus();
      } else if (!event.shiftKey && document.activeElement === tail) {
        event.preventDefault();
        head.focus();
      }
    };
    container.addEventListener("keydown", onKeyDown);

    return () => {
      container.removeEventListener("keydown", onKeyDown);
      // Only an element still in the document can take focus back; one that
      // left with the screen it belonged to would drop focus on the body.
      if (previous instanceof HTMLElement && previous.isConnected) {
        previous.focus({ preventScroll: true });
      }
    };
    // `initial` is read once, on open: focus is placed when the overlay
    // appears, not again whenever the caller re-renders.
  }, [ref]);
}

/** Whether a modal overlay is open, so window-wide accelerators stand aside. */
export function modalOpen(): boolean {
  return document.querySelector('[aria-modal="true"]') !== null;
}
