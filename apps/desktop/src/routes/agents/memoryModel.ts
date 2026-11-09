/**
 * What the memory section decides: which memories show, how a confidence is
 * said and drawn, and what an edit amounts to.
 */

import type { MemoryView } from "../../bindings/MemoryView";

/** The kinds of memory the runtime keeps, in the order the filter offers them. */
export const MEMORY_KINDS = [
  "fact",
  "decision",
  "preference",
  "task_history",
  "observation",
] as const;

/** The memories of `kind` (or every kind, for `null`) whose content or source contains `query`. */
export function filterMemories(
  memories: readonly MemoryView[],
  query: string,
  kind: string | null,
): MemoryView[] {
  const needle = query.trim().toLowerCase();
  return memories.filter(
    (memory) =>
      (kind === null || memory.kind === kind) &&
      (needle === "" ||
        memory.content.toLowerCase().includes(needle) ||
        memory.source.toLowerCase().includes(needle)),
  );
}

/** A confidence held to the range the runtime stores, or `null` when it is not a number. */
function clampConfidence(confidence: number): number | null {
  if (!Number.isFinite(confidence)) return null;
  return Math.min(1, Math.max(0, confidence));
}

/**
 * A confidence as the row prints it: two places at most, no trailing zeros.
 *
 * Clamped to the range the runtime stores, so a value that drifted outside it
 * on the wire is not printed as a confidence nobody could have set.
 */
export function confidenceLabel(confidence: number): string {
  const clamped = clampConfidence(confidence);
  if (clamped === null) return "confidence unknown";
  return `confidence ${Number(clamped.toFixed(2))}`;
}

/**
 * How full the confidence meter is drawn, as a whole percentage. Clamped as
 * the label is, so the bar and the words beside it always agree; an unknown
 * confidence draws an empty bar.
 */
export function confidencePercent(confidence: number): number {
  const clamped = clampConfidence(confidence);
  return clamped === null ? 0 : Math.round(clamped * 100);
}

/**
 * The band a confidence falls in, which colours its meter.
 *
 * Low confidence takes the attention hue: a note the agent half-believes is
 * still shown to it, and is worth a second look. The bands are coarse on
 * purpose; the number beside the bar is the precise answer.
 */
export function confidenceBand(confidence: number): "high" | "medium" | "low" {
  const clamped = clampConfidence(confidence);
  if (clamped === null || clamped < 0.5) return "low";
  return clamped < 0.8 ? "medium" : "high";
}

/** What the screen does when a memory edit is saved: `null` sends no change. */
export interface MemoryRevision {
  content: string;
  confidence: number | undefined;
}

/**
 * The revision an edit amounts to, or `null` when there is nothing to send.
 *
 * Empty content is refused here rather than sent: forgetting is its own
 * action, with its own question, and an emptied note is not a way round it.
 * The confidence is sent only when it changed, so an edit to the wording does
 * not restate a number the operator never touched.
 */
export function memoryRevision(
  memory: MemoryView,
  content: string,
  confidence: number,
): MemoryRevision | null {
  if (content.trim() === "") return null;
  const confidenceChanged = Math.abs(confidence - memory.confidence) > 1e-9;
  if (content === memory.content && !confidenceChanged) return null;
  return { content, confidence: confidenceChanged ? confidence : undefined };
}

/** How many characters of a memory a button's hidden name repeats. */
export const NAMED_CHARS = 40;

/**
 * Text cut to `max` characters, marked with an ellipsis when it was cut.
 *
 * Every memory row has an Edit and a Forget; a list of buttons read aloud
 * needs to say which memory each one is for, and a whole paragraph per button
 * would bury the verb.
 */
export function shortName(text: string, max = NAMED_CHARS): string {
  const trimmed = text.trim();
  if (trimmed.length <= max) return trimmed;
  return `${trimmed.slice(0, max - 1).trimEnd()}…`;
}
