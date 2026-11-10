/**
 * Where a run's time went: the three tracks the run timeline draws, computed
 * from the trace alone.
 *
 * Waiting time is counted once however the approvals overlapped, a call that
 * did not run is told apart by shape as well as tone, and anything outside
 * the run is clipped rather than allowed to stretch the chart.
 */

import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { TraceView } from "../../../bindings/TraceView";
import {
  type ChartTone,
  type HatchKind,
  type TrackMarker,
  type TrackSpan,
  elapsed,
} from "../../../components/charts";
import { outcomeTone } from "../../../components/status";
import { instant } from "../../../sdk/eventStream";
import { duration, humanise, isLive } from "../../../sdk/format";
import { truncate } from "../tasksModel";
import { type Entry, mergeEntries } from "./runModel";

/** What selecting a span on the timeline shows. */
export type SpanTarget =
  | { kind: "step"; ordinal: number }
  | { kind: "execution"; id: string }
  | { kind: "approval"; id: string };

/** A span on the timeline, and the row it belongs to. */
export interface TimelineSpan extends TrackSpan {
  target: SpanTarget;
}

/**
 * The hue of the line that marks the present on a live run: in-flight while
 * the run is working, the attention hue while it waits on a person — the same
 * split as the live dot in the run's eyebrow.
 */
export type NowTone = "info" | "warn";

/** Everything the run timeline draws, computed from the trace alone. */
export interface Timeline {
  start: number;
  end: number;
  /**
   * Whether the run is still going, so the chart's right edge is the present
   * moment rather than the run's end, and is drawn as such.
   */
  live: boolean;
  /** How the present is drawn while the run is live. */
  nowTone: NowTone;
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

/**
 * Whether the run is waiting on a person right now: it says so itself, or a
 * decision it asked for has not been given. The second matters in the moment
 * between an approval being raised and the run's state catching up.
 */
export function nowTone(trace: TraceView): NowTone {
  const waiting =
    trace.run.state === "waiting_for_approval" ||
    trace.approvals.some((approval) => approval.decided_at === null);
  return waiting ? "warn" : "info";
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
    live: isLive(trace.run.state) && !trace.run.completed_at,
    nowTone: nowTone(trace),
    model,
    tools,
    waiting,
    taint: Number.isFinite(taintAt) ? { at: taintAt, label: TAINT_LABEL } : null,
    waitingMs,
    summary: waitingSummary(waitingMs, window.end - window.start),
  };
}
