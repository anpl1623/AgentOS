/**
 * What the new-task composer decides, apart from how it draws it.
 *
 * The objective is a paragraph, not a line: Enter starts a new line, and
 * Command-Enter (Control-Enter elsewhere) sends it. Sending runs the task now,
 * or queues it when it waits for others, because a task that waits cannot
 * start at once and a Run that silently queued would claim it had.
 */

/** The parts of a key press the composer reads. */
export interface ComposerKey {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  /** True while an input method is composing; its Enter commits the composition. */
  isComposing: boolean;
}

/** What a key press in the objective asks for. */
export type SubmitIntent = "run" | "queue";

/**
 * Whether a key press sends the objective, and how.
 *
 * Either modifier is taken on any platform, so a hand used to the other
 * platform's chord is not ignored; Alt is left alone, since some keyboards
 * compose characters with it; and Enter that commits an input method's
 * composition is the composition's, never a send.
 */
export function submitIntent(event: ComposerKey, waits: number): SubmitIntent | null {
  if (event.key !== "Enter" || event.isComposing || event.altKey) return null;
  if (!event.metaKey && !event.ctrlKey) return null;
  return waits > 0 ? "queue" : "run";
}

/** Whether this is a Mac or an iPad, so the hint can say `⌘` where Command is meant. */
export function isApplePlatform(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

/** The send chord as a key cap shows it and as `aria-keyshortcuts` names it. */
export function submitKeys(apple: boolean): { cap: string; aria: string } {
  return apple
    ? { cap: "⌘ Enter", aria: "Meta+Enter" }
    : { cap: "Ctrl Enter", aria: "Control+Enter" };
}

/** The sentence under the objective, which says what sending will do. */
export function submitHint(waits: number, apple: boolean): string {
  const { cap } = submitKeys(apple);
  return waits > 0
    ? `Press ${cap} to queue it behind the tasks it waits for.`
    : `Press ${cap} to run it now.`;
}

/** The label of the control that opens the list of tasks to wait for. */
export function waitsLabel(waits: number): string {
  if (waits === 0) return "Wait for other tasks";
  return waits === 1 ? "Waits for 1 task" : `Waits for ${waits} tasks`;
}
