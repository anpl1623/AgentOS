/**
 * The window's keyboard accelerators, as a pure function of one key press.
 *
 * Two rules shape the table. No bare letter is bound: a screen binds its own
 * letters — the approvals queue moves with `j` and `k` — and a global grab
 * would fight it. And nothing here resolves an approval: approving is the one
 * action in this product that must cost a deliberate click, so no key, here or
 * in the palette, can do it.
 *
 * Everything is ignored while the operator is typing, except `Cmd/Ctrl+,`,
 * which a person reaches for mid-edit and which no text field uses.
 */

import type { RouteName } from "../routes/route";
import { NAV } from "./nav";

/** What a key press asks the shell to do. */
export type ShellCommand =
  | { kind: "screen"; route: RouteName }
  | { kind: "palette" }
  | { kind: "back" }
  | { kind: "forward" }
  | { kind: "shortcuts" };

/** The parts of a `KeyboardEvent` the table reads. */
export interface KeyPress {
  key: string;
  /** The physical key, so `Cmd+1` still works on layouts where 1 needs Shift. */
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * The command a key press asks for, or `null` when it is not an accelerator.
 *
 * `typing` is whether focus is in a text field, a select or editable content.
 * Command and Control are interchangeable, so the table needs no platform
 * check and a person who learned either finds it works.
 */
export function commandFor(press: KeyPress, typing: boolean): ShellCommand | null {
  const mod = press.metaKey || press.ctrlKey;

  if (mod && !press.altKey && !press.shiftKey) {
    if (press.key === ",") return { kind: "screen", route: "settings" };
    if (typing) return null;

    const digit = /^Digit([1-9])$/.exec(press.code)?.[1] ?? press.key;
    const item = /^[1-9]$/.test(digit) ? NAV[Number(digit) - 1] : undefined;
    if (item !== undefined) return { kind: "screen", route: item.route };

    if (press.key === "k" || press.key === "K") return { kind: "palette" };
    if (press.key === "[") return { kind: "back" };
    if (press.key === "]") return { kind: "forward" };
    return null;
  }

  if (typing) return null;

  // Alt with an arrow is the platform's own back and forward in a browser;
  // in a text field it moves by word, which is why typing is checked first.
  if (press.altKey && !mod && !press.shiftKey) {
    if (press.key === "ArrowLeft") return { kind: "back" };
    if (press.key === "ArrowRight") return { kind: "forward" };
    return null;
  }

  if (!mod && !press.altKey && press.key === "?") return { kind: "shortcuts" };
  return null;
}

/** Whether an event target is somewhere the operator types. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return target instanceof HTMLInputElement
    ? !NON_TEXT_INPUTS.has(target.type)
    : target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
}

/** Inputs that take no text, so an accelerator pressed on one is meant for the window. */
const NON_TEXT_INPUTS = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color"]);

/** Whether this is a Mac, so the sheet can say `⌘` where Command is meant. */
export function isApple(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

/** One line of the shortcut sheet. */
export interface ShortcutLine {
  /** Alternatives, each a sequence of keys. */
  keys: readonly (readonly string[])[];
  does: string;
}

/**
 * The shortcut sheet, written from the same facts {@link commandFor} reads so
 * the two cannot drift.
 */
export function shortcutLines(apple: boolean): ShortcutLine[] {
  const mod = apple ? "⌘" : "Ctrl";
  return [
    ...NAV.map((item, index) => ({
      keys: [[mod, String(index + 1)]],
      does: `Go to ${item.label}`,
    })),
    { keys: [[mod, ","]], does: "Go to Settings, even while typing" },
    { keys: [[mod, "K"]], does: "Open or close the palette" },
    { keys: [[mod, "["], [apple ? "⌥" : "Alt", "←"]], does: "Back" },
    { keys: [[mod, "]"], [apple ? "⌥" : "Alt", "→"]], does: "Forward" },
    { keys: [["?"]], does: "Show this sheet" },
    { keys: [["Esc"]], does: "Close the palette, this sheet or a question" },
  ];
}
