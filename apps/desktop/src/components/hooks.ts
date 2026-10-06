/**
 * Small hooks every screen needs, kept in one place so no screen grows a
 * private copy with slightly different behaviour.
 */

import { useCallback, useEffect, useRef, useState } from "react";

/**
 * The current time in milliseconds, refreshed every `intervalMs`.
 *
 * Minute resolution by default. Relative times on this app read "4m ago", and
 * a component that re-renders every second to say the same thing sixty times
 * costs work and, on an approval card, moves text under the reader's eye while
 * they are deciding whether to let something run.
 */
export function useNow(intervalMs = 60_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);
  return now;
}

// String sequences: OSC, DCS, SOS, PM and APC, each running to a terminator.
// BEL is strictly an OSC terminator, but xterm accepts it for all of them and
// being generous here only removes more. An unterminated sequence is left for
// the control-character pass, which takes its introducer and leaves the rest
// as inert visible text rather than silently swallowing everything after it.
const STRING_SEQUENCE =
  /(?:\u001b[\]PX^_]|[\u0090\u0098\u009d\u009e\u009f])[\s\S]*?(?:\u0007|\u001b\\|\u009c)/g;

// Control sequences: CSI, in its 7-bit and 8-bit forms, with parameter and
// intermediate bytes and one final byte. This is where colour, cursor
// movement, screen clearing and bracketed-paste toggles live.
const CONTROL_SEQUENCE = /(?:\u001b\[|\u009b)[0-?]*[ -/]*[@-~]/g;

// Every other escape: ESC, any intermediates, one final byte. Covers charset
// switches and the full-reset `ESC c`.
const ESCAPE_SEQUENCE = /\u001b[ -/]*[0-~]/g;

// C0 and C1 controls and DEL, keeping tab and line feed. Carriage return goes
// too: a bare CR lets later text overwrite earlier text on a terminal, so what
// the operator sees after pasting would not be what they copied.
const CONTROL_CHARACTER = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g;

/**
 * Makes text safe to paste into a terminal.
 *
 * Operators copy tool output, and tool output is untrusted: an agent can be
 * induced to fetch a page whose text contains escape sequences. Pasted into a
 * terminal, those can recolour or erase what is on screen, rewrite the title,
 * or end a bracketed paste early so the remainder runs as typed input. This
 * removes ANSI escape sequences whole, then every remaining C0 and C1 control
 * character except `\n` and `\t`. What survives is the text a person could
 * have read in the window.
 */
export function sanitiseForClipboard(text: string): string {
  return text
    .replace(STRING_SEQUENCE, "")
    .replace(CONTROL_SEQUENCE, "")
    .replace(ESCAPE_SEQUENCE, "")
    .replace(CONTROL_CHARACTER, "");
}

/** Where a copy stands. `copied` and `failed` fall back to `idle` after a moment. */
export type CopyState = "idle" | "copied" | "failed";

/** How long a copy's result stays on screen before the control resets. */
const COPY_FEEDBACK_MS = 2_000;

/**
 * Copies sanitised text to the clipboard and reports whether it worked.
 *
 * `copy` resolves `true` on success. `state` carries the same answer for a
 * control to render, so a failure is shown rather than assumed away; a
 * clipboard the webview refused is a real outcome on some platforms.
 */
export function useCopy(): { copy: (text: string) => Promise<boolean>; state: CopyState } {
  const [state, setState] = useState<CopyState>("idle");
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = useCallback(async (text: string) => {
    const ok = await writeClipboard(sanitiseForClipboard(text));
    setState(ok ? "copied" : "failed");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setState("idle"), COPY_FEEDBACK_MS);
    return ok;
  }, []);

  return { copy, state };
}

async function writeClipboard(text: string): Promise<boolean> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // A denied permission or an unfocused document; the older path below
      // is governed differently and may still succeed.
    }
  }
  return copyThroughSelection(text);
}

/**
 * The pre-Clipboard-API route: select a hidden textarea and ask the document
 * to copy. Focus is returned to wherever it was, because selecting the
 * textarea takes it, and a keyboard user who pressed Copy should not find
 * themselves at the top of the page.
 */
function copyThroughSelection(text: string): boolean {
  const previous = document.activeElement;
  const area = document.createElement("textarea");
  area.value = text;
  area.readOnly = true;
  area.setAttribute("aria-hidden", "true");
  area.className = "visually-hidden";
  document.body.appendChild(area);
  try {
    area.select();
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    area.remove();
    if (previous instanceof HTMLElement) previous.focus();
  }
}
