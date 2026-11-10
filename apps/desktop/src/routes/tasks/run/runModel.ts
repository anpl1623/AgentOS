/**
 * What the run screen decides about the trace itself, apart from how it draws
 * it.
 *
 * The trace is what an operator opens after something went wrong, and what
 * they copy out of the window as evidence. Every rule here is about not
 * misreporting that: entries in the order they happened, a stop that is not
 * yet a stop shown as such, and a failure pinned to what produced it. The
 * rest of the screen's decisions are beside this file, one concern each:
 * keeping up with the run (`refreshModel`), delegation (`delegationModel`),
 * the timeline (`timelineModel`), the copied transcript (`transcriptModel`)
 * and how each entry looks (`traceLook`).
 */

import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { RunSummary } from "../../../bindings/RunSummary";
import type { StepView } from "../../../bindings/StepView";
import { instant } from "../../../sdk/eventStream";
import { isLive } from "../../../sdk/format";

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

/** One line of the merged trace: a step, or a tool call, at the time it happened. */
export type Entry =
  | { kind: "step"; key: string; at: string; ms: number; step: StepView }
  | { kind: "execution"; key: string; at: string; ms: number; execution: ExecutionView };

/** The three ways the trace can be read. */
export type TraceMode = "timeline" | "calls" | "steps";

/**
 * Steps and tool calls as one list, in the order they happened.
 *
 * Sorted by parsed time. Among entries at the same instant a tool call comes
 * before a step, because the step that reports a call is written after the
 * call it reports; then steps by ordinal and calls in the order the runtime
 * listed them. A time that cannot be read sorts last rather than poisoning the
 * comparison, keeping the same tiebreak among such entries.
 */
export function mergeEntries(
  steps: readonly StepView[],
  executions: readonly ExecutionView[],
): Entry[] {
  const ranked = [
    ...executions.map((execution, index) => ({
      entry: {
        kind: "execution" as const,
        key: `execution-${execution.id}`,
        at: execution.started_at,
        ms: instant(execution.started_at),
        execution,
      },
      group: 0,
      order: index,
    })),
    ...steps.map((step) => ({
      entry: {
        kind: "step" as const,
        key: `step-${step.ordinal}`,
        at: step.at,
        ms: instant(step.at),
        step,
      },
      group: 1,
      order: step.ordinal,
    })),
  ];
  ranked.sort(
    (a, b) =>
      compareInstants(a.entry.ms, b.entry.ms) || a.group - b.group || a.order - b.order,
  );
  return ranked.map((item) => item.entry);
}

/** Orders two instants, unreadable (infinite) ones last and equal to each other. */
export function compareInstants(a: number, b: number): number {
  if (a === b) return 0;
  if (!Number.isFinite(a)) return Number.isFinite(b) ? 1 : 0;
  if (!Number.isFinite(b)) return -1;
  return a - b;
}

/** The entries a mode shows. */
export function entriesFor(mode: TraceMode, entries: readonly Entry[]): Entry[] {
  if (mode === "calls") return entries.filter((entry) => entry.kind === "execution");
  if (mode === "steps") return entries.filter((entry) => entry.kind === "step");
  return [...entries];
}

/** A step's marker: a glyph for the eye, and the words it stands for. */
export interface Marker {
  glyph: string;
  label: string;
}

/**
 * The marker for a step kind.
 *
 * The glyph alone is not the meaning: it is drawn hidden from assistive
 * technology, and the label is what a screen reader hears before the summary.
 */
export function marker(kind: string): Marker {
  switch (kind) {
    case "toolcall":
    case "tool_call":
      return { glyph: "▶", label: "tool call" };
    case "approval":
      return { glyph: "?", label: "approval" };
    case "verification":
      return { glyph: "✓", label: "verification" };
    case "recovery":
      return { glyph: "↻", label: "recovery" };
    default:
      return { glyph: "◆", label: "step" };
  }
}

// ---------------------------------------------------------------------------
// The run's state and the actions on it
// ---------------------------------------------------------------------------

/**
 * The state to show, given whether the operator has asked the run to stop.
 *
 * `cancelling` until the runtime reports the run ended. A tool call in flight
 * finishes or is abandoned on the runtime's schedule, not the button's, and
 * the screen must not say the agent has stopped while it may still be acting.
 */
export function shownState(state: string, cancelRequested: boolean): string {
  return cancelRequested && isLive(state) ? "cancelling" : state;
}

/**
 * Whether this attempt can be retried from here.
 *
 * Only a failed or cancelled attempt, and only the latest: the runtime retries
 * a task's latest attempt, so a Retry beside an older one would start a run
 * after an attempt the operator is not looking at. `latestId` is `null` while
 * the attempts are unknown; the shown attempt is then taken as the latest, and
 * the runtime's refusal is the backstop.
 *
 * Never a run another agent delegated. Its task is carried out only inside the
 * run that asked for it, under that run's policy as well as its own, and the
 * runtime refuses to start it alone; a button that could only be refused is
 * not offered.
 */
export function canRetry(run: RunSummary, latestId: string | null, delegated: boolean): boolean {
  const ended = run.state === "failed" || run.state === "cancelled";
  return !delegated && ended && (latestId === null || latestId === run.id);
}

/** `attempt {n} of {total}`, once there is more than one. */
export function attemptsLabel(run: RunSummary, runs: readonly RunSummary[]): string | null {
  return runs.length > 1 ? `attempt ${run.attempt} of ${runs.length}` : null;
}

/**
 * A decided request's status as a verdict tone, as the approvals queue gives
 * it. Written here rather than imported from that screen, so this screen
 * depends on no other screen's internals.
 */
function decisionTone(status: string): "ok" | "blocked" | "neutral" {
  if (status === "approved") return "ok";
  if (status === "denied") return "blocked";
  return "neutral";
}

/** An approval's status as a verdict tone; one still pending is live work. */
export function approvalTone(status: string): "ok" | "blocked" | "neutral" | "live" {
  return status === "pending" ? "live" : decisionTone(status);
}

// ---------------------------------------------------------------------------
// Where a failure happened
// ---------------------------------------------------------------------------

/** The entry a run's failure is pinned to. */
export type FailureAnchor = { kind: "execution"; id: string } | { kind: "step"; ordinal: number };

const TOOL_FAILURE = /^tool `([^`]+)` failed: ([\s\S]*)$/;

/**
 * The step or tool call that produced the run's failure, when the trace says.
 *
 * A tool failure names its tool, and is pinned to that tool's last failing
 * call, preferring one whose error carries the same message. Any other
 * failure the runtime recovers from first is reported by a recovery step that
 * quotes it, and is pinned to the last such step. Otherwise nothing in the
 * trace produced it — a step budget, a timeout, the process ending — and it
 * stays the banner at the top, with no anchor rather than a guessed one.
 */
export function failureAnchor(
  run: RunSummary,
  steps: readonly StepView[],
  executions: readonly ExecutionView[],
): FailureAnchor | null {
  const failure = run.failure?.trim();
  if (!failure) return null;

  const tool = TOOL_FAILURE.exec(failure);
  if (tool) {
    const [, name, message] = tool;
    const failing = executions.filter(
      (execution) => execution.tool === name && execution.outcome !== "success",
    );
    const exact = failing.filter((execution) => execution.error?.includes(message ?? "") ?? false);
    const chosen = exact.at(-1) ?? failing.at(-1);
    if (chosen) return { kind: "execution", id: chosen.id };
  }

  const quoting = steps.filter(
    (step) => step.kind === "recovery" && step.summary.includes(failure),
  );
  const step = quoting.at(-1);
  return step ? { kind: "step", ordinal: step.ordinal } : null;
}
