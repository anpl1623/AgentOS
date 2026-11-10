/**
 * The window-wide accelerators, bound to the shell.
 *
 * What a key press means is decided in `keys.ts`; this only listens, stands
 * aside while a modal is open, and carries the command out.
 */

import { useEffect } from "react";

import type { Navigate } from "../routes/route";
import { commandFor, isTypingTarget } from "./keys";
import { modalOpen } from "./modal";
import { screenRoute } from "./nav";

/** The shell's overlays that a key can open. */
export type KeyOverlay = "palette" | "shortcuts";

export function useShellKeys({
  navigate,
  back,
  forward,
  open,
}: {
  navigate: Navigate;
  back: () => void;
  forward: () => void;
  open: (overlay: KeyOverlay) => void;
}): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || modalOpen()) return;
      const command = commandFor(event, isTypingTarget(event.target));
      if (command === null) return;
      event.preventDefault();
      switch (command.kind) {
        case "screen":
          navigate(screenRoute(command.route));
          break;
        case "palette":
          open("palette");
          break;
        case "shortcuts":
          open("shortcuts");
          break;
        case "back":
          back();
          break;
        case "forward":
          forward();
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate, back, forward, open]);
}
