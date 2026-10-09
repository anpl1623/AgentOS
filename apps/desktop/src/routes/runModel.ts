/**
 * What the run screen decides, apart from how it draws it.
 *
 * The trace is what an operator opens after something went wrong, and what
 * they copy out of the window as evidence. Every rule here is about not
 * misreporting that: entries in the order they happened, a stop that is not
 * yet a stop shown as such, a failure pinned to what produced it, waiting time
 * counted once however the approvals overlapped, and a transcript in which a
 * hostile string cannot rearrange itself on paste.
 */

import type { EventView } from "../bindings/EventView";
import type { ExecutionView } from "../bindings/ExecutionView";
import type { RunSummary } from "../bindings/RunSummary";
import type { StepView } from "../bindings/StepView";
import type { TraceView } from "../bindings/TraceView";
import { visibleText } from "../components/argumentText";
import {
  type ChartTone,
  type HatchKind,
  type TrackMarker,
  type TrackSpan,
  elapsed,
} from "../components/charts";
import { outcomeTone } from "../components/status";
import { instant } from "../sdk/eventStream";
import { duration, humanise, isLive } from "../sdk/format";
import { decisionTone } from "./approvalsModel";
import { truncate } from "./tasksModel";

// ---------------------------------------------------------------------------
// Refresh
// ---------------------------------------------------------------------------

/**
 * How long a burst of events for this run is gathered before one reload.
 *
 * One model turn emits a step, an execution, a permission decision and often
 * an approval within a few milliseconds. Reloading on each would fetch the
 * whole trace four or five times for one change.
 */
export const REFRESH_DEBOUNCE_MS = 150;

/**
 * The longest a burst may hold its reload back.
 *
 * A run that emits an event every hundred milliseconds would otherwise keep a
 * trailing debounce waiting for a quiet moment that never comes, and the trace
 * would stand still while the run moved.
 */
export const REFRESH_MAX_WAIT_MS = 1_000;

/**
 * The safety-net reload while a run is live.
 *
 * The event stream is lossy under lag — the runtime drops what a slow window
 * cannot take — so an event-driven screen still re-reads on a timer. Slowed,
 * never removed.
 */
export const LIVE_INTERVAL_MS = 10_000;

/** The reload once a run has ended; only a retry elsewhere or a late record changes it. */
export const SETTLED_INTERVAL_MS = 60_000;

/**
 * Whether a streamed event concerns this run.
 *
 * The payload arrives from the runtime untyped, so its shape is checked
 * rather than assumed.
 */
export function concernsRun(payload: unknown, runId: string): boolean {
  return (
    typeof payload === "object" &&
    payload !== null &&
    (payload as Partial<EventView>).run_id === runId
  );
}

/**
 * A trailing debounce: `schedule` as often as you like, `fn` runs once after a
 * quiet `waitMs`. `flush` runs it now, in place of anything pending.
 */
export interface Coalescer {
  schedule: () => void;
  flush: () => void;
  cancel: () => void;
}

/**
 * Coalesce calls into one, `waitMs` after the last of them, and never more
 * than `maxWaitMs` after the first.
 *
 * Trailing rather than leading: the first event of a model turn is the least
 * informative of its burst, and a reload fired on it would miss the rest and
 * need another. Bounded, so that a burst which never pauses still reloads.
 */
export function coalesce(fn: () => void, waitMs: number, maxWaitMs = Infinity): Coalescer {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let since: number | undefined;
  const cancel = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    since = undefined;
  };
  const flush = () => {
    cancel();
    fn();
  };
  return {
    schedule: () => {
      const now = Date.now();
      since ??= now;
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(flush, Math.max(0, Math.min(waitMs, since + maxWaitMs - now)));
    },
    flush,
    cancel,
  };
}

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
function compareInstants(a: number, b: number): number {
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
 */
export function canRetry(run: RunSummary, latestId: string | null): boolean {
  const ended = run.state === "failed" || run.state === "cancelled";
  return ended && (latestId === null || latestId === run.id);
}

/** `attempt {n} of {total}`, once there is more than one. */
export function attemptsLabel(run: RunSummary, runs: readonly RunSummary[]): string | null {
  return runs.length > 1 ? `attempt ${run.attempt} of ${runs.length}` : null;
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

// ---------------------------------------------------------------------------
// The timeline
// ---------------------------------------------------------------------------

/** What selecting a span on the timeline shows. */
export type SpanTarget =
  | { kind: "step"; ordinal: number }
  | { kind: "execution"; id: string }
  | { kind: "approval"; id: string };

/** A span on the timeline, and the row it belongs to. */
export interface TimelineSpan extends TrackSpan {
  target: SpanTarget;
}

/** Everything the run timeline draws, computed from the trace alone. */
export interface Timeline {
  start: number;
  end: number;
  model: TimelineSpan[];
  tools: TimelineSpan[];
  waiting: TimelineSpan[];
  /** The first call made after the run read untrusted data. */
  taint: TrackMarker | null;
  /** Time spent waiting for a decision, overlaps counted once. */
  waitingMs: number;
  summary: string;
}

/** The words the taint marker uses, the same as the `Tainted` chip's. */
export const TAINT_LABEL = "read untrusted data";

/** Step kinds that stand for a model turn. */
const MODEL_STEPS = new Set(["planning", "verification"]);

/** Outcomes that are a refusal rather than some other failure to run. */
const REFUSALS = new Set(["denied", "approval_denied"]);

/** A verdict tone as a chart tone. */
function chartTone(tone: string): ChartTone {
  switch (tone) {
    case "ok":
    case "danger":
    case "blocked":
      return tone;
    case "live":
      return "info";
    default:
      return "neutral";
  }
}

/**
 * The tone and hatch a tool call is drawn with.
 *
 * Toned by whether it executed: an executed call is ok or, if it failed,
 * danger. One that did not execute takes its outcome's own tone — a refusal
 * the accent of the policy working — and is hatched, a refusal one way and
 * anything else that did not run the other.
 */
export function toolLook(execution: ExecutionView): { tone: ChartTone; hatch: HatchKind | null } {
  if (execution.executed) {
    return { tone: execution.outcome === "success" ? "ok" : "danger", hatch: null };
  }
  return {
    tone: chartTone(outcomeTone(execution.outcome)),
    hatch: REFUSALS.has(execution.outcome) ? "refused" : "skipped",
  };
}

/**
 * The run's extent: from its start to its completion, or to `now` while it is
 * live. A run that ended without recording when is taken to have ended at the
 * last thing it recorded. `null` when its start cannot be read or it has no
 * length.
 */
export function runWindow(trace: TraceView, now: number): { start: number; end: number } | null {
  const { run } = trace;
  const start = instant(run.started_at);
  if (!Number.isFinite(start)) return null;
  let end = run.completed_at ? instant(run.completed_at) : Number.NaN;
  if (!Number.isFinite(end)) {
    if (isLive(run.state)) end = now;
    else {
      const recorded = [
        ...trace.steps.map((step) => instant(step.at)),
        ...trace.executions.map(
          (execution) => instant(execution.started_at) + execution.duration_ms,
        ),
        ...trace.approvals.map((approval) => instant(approval.decided_at ?? approval.requested_at)),
      ].filter(Number.isFinite);
      end = recorded.length > 0 ? Math.max(...recorded) : start;
    }
  }
  return end > start ? { start, end } : null;
}

/** Clip an interval to a window; `null` when nothing of it is inside or it cannot be read. */
function clip(
  from: number,
  to: number,
  window: { start: number; end: number },
): { from: number; to: number } | null {
  if (!Number.isFinite(from) || !Number.isFinite(to)) return null;
  const a = Math.max(from, window.start);
  const b = Math.min(to, window.end);
  return b >= a ? { from: a, to: b } : null;
}

/**
 * The total length of a set of intervals, with overlaps counted once.
 *
 * Two approvals raised by parallel calls wait at the same time; adding their
 * lengths would report more waiting than the run spent.
 */
export function unionLength(intervals: readonly { from: number; to: number }[]): number {
  const sorted = intervals.filter((each) => each.to > each.from).sort((a, b) => a.from - b.from);
  let total = 0;
  let open: { from: number; to: number } | null = null;
  for (const interval of sorted) {
    if (open !== null && interval.from <= open.to) {
      open.to = Math.max(open.to, interval.to);
      continue;
    }
    if (open !== null) total += open.to - open.from;
    open = { ...interval };
  }
  if (open !== null) total += open.to - open.from;
  return total;
}

/** The plain sentence under the tracks. */
export function waitingSummary(waitingMs: number, runMs: number): string {
  if (waitingMs <= 0) return "The run did not wait for a decision.";
  return `${elapsed(waitingMs)} of the run's ${elapsed(runMs)} was spent waiting for a decision.`;
}

/**
 * The timeline's three tracks, or `null` when there is nothing to draw: no
 * readable extent, or not one step, call or approval.
 *
 * Model turns are inferred: a planning or verification step runs to the next
 * step of any kind, or to the run's end. The runtime records when a step was
 * written, not how long the model took, and the chart's caption says so.
 * Tool calls are as the runtime timed them. Waiting runs from when a decision
 * was asked for to when it was given, or to the run's end while it is not.
 */
export function buildTimeline(trace: TraceView, now: number): Timeline | null {
  if (trace.steps.length + trace.executions.length + trace.approvals.length === 0) return null;
  const window = runWindow(trace, now);
  if (window === null) return null;

  const steps = mergeEntries(trace.steps, [])
    .filter((entry): entry is Extract<Entry, { kind: "step" }> => entry.kind === "step")
    .map((entry) => entry.step);
  const model: TimelineSpan[] = [];
  steps.forEach((step, index) => {
    if (!MODEL_STEPS.has(step.kind)) return;
    const next = steps[index + 1];
    const range = clip(instant(step.at), next ? instant(next.at) : window.end, window);
    if (range === null) return;
    model.push({
      id: `step-${step.ordinal}`,
      ...range,
      tone: "info",
      hatch: null,
      label:
        `Model turn from step ${step.ordinal}, about ${elapsed(range.to - range.from)}: ` +
        truncate(step.summary, 80),
      target: { kind: "step", ordinal: step.ordinal },
    });
  });

  const calls = mergeEntries([], trace.executions)
    .filter(
      (entry): entry is Extract<Entry, { kind: "execution" }> => entry.kind === "execution",
    )
    .map((entry) => entry.execution);
  const tools: TimelineSpan[] = [];
  for (const execution of calls) {
    const from = instant(execution.started_at);
    const range = clip(from, from + execution.duration_ms, window);
    if (range === null) continue;
    tools.push({
      id: execution.id,
      ...range,
      ...toolLook(execution),
      label:
        `${execution.tool}, ${humanise(execution.outcome)}, ${duration(execution.duration_ms)}` +
        (execution.executed ? "" : ", did not execute"),
      target: { kind: "execution", id: execution.id },
    });
  }

  const tainted = calls.find((execution) => execution.tainted);
  const taintAt = tainted ? instant(tainted.started_at) : Number.NaN;

  const waiting: TimelineSpan[] = [];
  for (const approval of trace.approvals) {
    const decided = approval.decided_at ? instant(approval.decided_at) : window.end;
    const range = clip(instant(approval.requested_at), decided, window);
    if (range === null) continue;
    const verb = approval.decided_at ? `${approval.status} after` : "still waiting after";
    waiting.push({
      id: approval.id,
      ...range,
      tone: "warn",
      hatch: "waiting",
      label: `Waiting on you for ${approval.tool}, ${verb} ${elapsed(range.to - range.from)}`,
      target: { kind: "approval", id: approval.id },
    });
  }

  const waitingMs = unionLength(waiting);
  return {
    ...window,
    model,
    tools,
    waiting,
    taint: Number.isFinite(taintAt) ? { at: taintAt, label: TAINT_LABEL } : null,
    waitingMs,
    summary: waitingSummary(waitingMs, window.end - window.start),
  };
}

// ---------------------------------------------------------------------------
// The transcript
// ---------------------------------------------------------------------------

/**
 * A time as `HH:MM:SS` on the local 24-hour clock, fixed rather than in the
 * locale's format, so a pasted transcript reads the same on every machine. A
 * time that cannot be read is given as it was written.
 */
export function hhmmss(iso: string): string {
  const ms = instant(iso);
  if (!Number.isFinite(ms)) return iso;
  const at = new Date(ms);
  return [at.getHours(), at.getMinutes(), at.getSeconds()]
    .map((part) => String(part).padStart(2, "0"))
    .join(":");
}

/**
 * Text from the run, safe to put in a transcript.
 *
 * The summaries and errors are the model's and the tools' words, and a tool
 * may have read a hostile page. A direction override pasted into a ticket
 * reorders what the reader sees, so every invisible or direction-changing
 * code point is written as its visible escape, the same one the argument
 * block draws. Control characters are removed later, by the copy itself.
 */
function plain(text: string): string {
  return visibleText(text);
}

/** Lines after the first, indented under the line they belong to. */
function continued(text: string, indent: string): string {
  return text
    .split("\n")
    .map((line, index) => (index === 0 ? line : `${indent}${line}`))
    .join("\n");
}

const INDENT = "    ";

/** One entry as one transcript line, and its error indented beneath. */
function entryLine(entry: Entry): string {
  if (entry.kind === "step") {
    const { step } = entry;
    return `${hhmmss(step.at)} ${step.kind} ${continued(plain(step.summary), INDENT)}`;
  }
  const { execution } = entry;
  const line =
    `${hhmmss(execution.started_at)} tool ${plain(execution.tool)} ` +
    `decision=${execution.effect} outcome=${execution.outcome}`;
  return execution.error
    ? `${line}\n${INDENT}${continued(plain(execution.error), INDENT)}`
    : line;
}

/**
 * The run as plain text, for pasting into a ticket or a message.
 *
 * Objective, agent, run, attempt, state, taint and tokens, then the failure or
 * result when there is one, then one line per entry of the merged timeline:
 * `HH:MM:SS kind summary`, a tool call annotated with its decision and
 * outcome, and an error indented beneath the call that raised it.
 */
export function transcript(trace: TraceView, shown: string = trace.run.state): string {
  const { run } = trace;
  const taint = run.tainted
    ? run.taint_sources.length > 0
      ? `${TAINT_LABEL} from ${run.taint_sources.map(plain).join(", ")}`
      : TAINT_LABEL
    : "none";
  const head = [
    `Objective: ${continued(plain(trace.objective), INDENT)}`,
    `Agent: ${plain(trace.agent_name)}`,
    `Run: ${run.id}`,
    `Attempt: ${run.attempt}`,
    `State: ${humanise(shown)}`,
    `Taint: ${taint}`,
    `Tokens: ${run.input_tokens} in, ${run.output_tokens} out`,
  ];
  if (run.failure) head.push(`Failure: ${continued(plain(run.failure), INDENT)}`);
  if (run.result) head.push(`Result: ${continued(plain(run.result), INDENT)}`);
  const entries = mergeEntries(trace.steps, trace.executions).map(entryLine);
  return `${[...head, "", ...entries].join("\n")}\n`;
}

