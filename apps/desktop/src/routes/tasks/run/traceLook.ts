/**
 * How the run screen draws what the trace says: the icon of a tool's family,
 * the node a step or call hangs on, the tone of the run's state, and the
 * figures in its header.
 *
 * Presentation, but still decisions, and each one could misreport: a call
 * drawn in the colour of success that was refused, a dot that pulses beside a
 * run that has ended, a waiting run drawn as calm. So they live here, held by
 * tests, and the components only draw what they are given.
 */

import type { ApprovalView } from "../../../bindings/ApprovalView";
import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { RunSummary } from "../../../bindings/RunSummary";
import type { ChartTone } from "../../../components/charts";
import type { IconName } from "../../../components/icons";
import type { LiveTone } from "../../../components/LiveDot";
import type { StatTone } from "../../../components/StatTile";
import { instant } from "../../../sdk/eventStream";
import { humanise, isLive } from "../../../sdk/format";
import type { Entry, TraceMode } from "./runModel";
import { toolLook } from "./timelineModel";

/** A wire value as chip text, as the status chips write theirs. */
export function sentence(value: string): string {
  const words = humanise(value);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

// ---------------------------------------------------------------------------
// Tool families
// ---------------------------------------------------------------------------

/** The family a tool belongs to, named by the part of its name before the first dot. */
export interface ToolFamily {
  key: string;
  label: string;
  icon: IconName;
}

const FAMILIES = {
  filesystem: { label: "Filesystem", icon: "folder" },
  browser: { label: "Browser", icon: "window" },
  terminal: { label: "Terminal", icon: "terminal" },
  network: { label: "Network", icon: "globe" },
  computer: { label: "Computer", icon: "monitor" },
  vision: { label: "Vision", icon: "eye" },
  github: { label: "GitHub", icon: "link" },
  agent: { label: "Delegation", icon: "agents" },
} as const satisfies Record<string, { label: string; icon: IconName }>;

/**
 * A tool's family: its icon and the word for it.
 *
 * The icon is there to be scanned — a column of folders and terminals reads
 * faster than a column of dotted names — and never replaces the name, which
 * is always drawn beside it. A family this build does not know keeps its own
 * word and a generic mark, rather than borrowing the look of one it does.
 * Looked up as an own key, so a tool named `constructor.x` is not mistaken for
 * a family.
 */
export function toolFamily(tool: string): ToolFamily {
  const key = (tool.split(".")[0] ?? "").trim().toLowerCase();
  if (Object.hasOwn(FAMILIES, key)) {
    return { key, ...FAMILIES[key as keyof typeof FAMILIES] };
  }
  return key === ""
    ? { key: "tool", label: "Tool", icon: "zap" }
    : { key, label: sentence(key), icon: "zap" };
}

// ---------------------------------------------------------------------------
// Nodes on the trace
// ---------------------------------------------------------------------------

/** How a node on the trace's rail is drawn: its icon, its tone, and the word over the line. */
export interface NodeLook {
  icon: IconName;
  tone: ChartTone;
  /** Drawn for the eye only; a screen reader hears the step's marker label instead. */
  label: string;
}

/**
 * A tool call's node: its family's icon, toned as the timeline tones its span,
 * so a refused call is the accent of the policy working wherever it is drawn.
 */
export function callLook(execution: ExecutionView): NodeLook {
  const family = toolFamily(execution.tool);
  return { icon: family.icon, tone: toolLook(execution).tone, label: family.label };
}

/**
 * A step's node.
 *
 * A model turn is the accent of something thinking; a step that reports a
 * call takes that call's look, when the call is known, so the two read as one
 * event; recovery and approval are the steps an operator looks for after a
 * failure, and take the attention hue. Anything else is neutral and named by
 * its own kind.
 */
export function stepLook(kind: string, execution: ExecutionView | null = null): NodeLook {
  switch (kind) {
    case "planning":
      return { icon: "sparkles", tone: "info", label: "Planning" };
    case "verification":
      return { icon: "check", tone: "info", label: "Verification" };
    case "toolcall":
    case "tool_call":
      return execution
        ? { ...callLook(execution), label: "Tool call" }
        : { icon: "play", tone: "neutral", label: "Tool call" };
    case "approval":
      return { icon: "approvals", tone: "warn", label: "Approval" };
    case "recovery":
      return { icon: "retry", tone: "warn", label: "Recovery" };
    default:
      return { icon: "info", tone: "neutral", label: kind.trim() === "" ? "Step" : sentence(kind) };
  }
}

/**
 * The keys of the entries made after the run first read untrusted data: the
 * first call the runtime marked tainted, and everything after it.
 *
 * The rail beneath them is drawn as tainted, in the attention hue and dashed,
 * matching the marker on the timeline. Counted over every entry, not the ones
 * a mode shows, so the Steps mode marks the same moment the Timeline mode does.
 */
export function taintedKeys(entries: readonly Entry[]): Set<string> {
  const first = entries.findIndex(
    (entry) => entry.kind === "execution" && entry.execution.tainted,
  );
  return new Set(first === -1 ? [] : entries.slice(first).map((entry) => entry.key));
}

/** How many entries each mode shows, for the counts on the mode switch. */
export function modeCounts(entries: readonly Entry[]): Record<TraceMode, number> {
  const calls = entries.filter((entry) => entry.kind === "execution").length;
  return { timeline: entries.length, calls, steps: entries.length - calls };
}

/** Every tool call's id, for opening or closing all of them at once. */
export function callIds(entries: readonly Entry[]): string[] {
  return entries.flatMap((entry) => (entry.kind === "execution" ? [entry.execution.id] : []));
}

/** Whether every call in `ids` is open; `false` when there are none to open. */
export function allOpen(open: ReadonlySet<string>, ids: readonly string[]): boolean {
  return ids.length > 0 && ids.every((id) => open.has(id));
}

// ---------------------------------------------------------------------------
// The run's header
// ---------------------------------------------------------------------------

/**
 * The live dot's hue for a run, or `null` for one that has ended.
 *
 * A run waiting on a person, or asked to stop and not yet stopped, is in the
 * attention hue: both are waiting on something other than the model. Anything
 * else still moving is the hue of a run in flight.
 */
export function liveTone(shown: string): LiveTone | null {
  if (shown === "cancelling" || shown === "waiting_for_approval") return "warn";
  return isLive(shown) ? "info" : null;
}

/**
 * The tone of the state tile.
 *
 * Loud only where the run needs a person: waiting on a decision, asked to stop,
 * or failed. A finished run is quiet, and a cancelled one quieter still.
 */
export function stateTone(shown: string): StatTone {
  switch (shown) {
    case "waiting_for_approval":
    case "cancelling":
      return "warn";
    case "failed":
      return "danger";
    case "completed":
      return "ok";
    case "cancelled":
    case "idle":
      return "accent";
    default:
      return isLive(shown) ? "info" : "accent";
  }
}

/** The state tile's icon: a shape for each outcome, so the tone is never the only signal. */
export function stateIcon(shown: string): IconName {
  switch (shown) {
    case "waiting_for_approval":
      return "approvals";
    case "cancelling":
    case "cancelled":
      return "stop";
    case "failed":
      return "x";
    case "completed":
      return "check";
    default:
      return isLive(shown) ? "zap" : "info";
  }
}

/**
 * How long the run has taken: to its end, or to `now` while it is live.
 *
 * `null` when that cannot be said — a start that cannot be read, or a run that
 * ended without recording when — rather than a figure counted to the present
 * for a run that is not in it.
 */
export function runElapsed(run: RunSummary, now: number): number | null {
  const start = instant(run.started_at);
  if (!Number.isFinite(start)) return null;
  const end = run.completed_at ? instant(run.completed_at) : isLive(run.state) ? now : Number.NaN;
  if (!Number.isFinite(end)) return null;
  return Math.max(0, end - start);
}

/** The approvals this run is still waiting on, in the order it asked. */
export function pendingApprovals(approvals: readonly ApprovalView[]): ApprovalView[] {
  return approvals.filter((approval) => approval.status === "pending");
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/** How many leading characters of a hash are shown; the whole value is a copy away. */
export const HASH_PREFIX = 16;

/**
 * The first {@link HASH_PREFIX} characters of a hash, marked as cut when they
 * are; the same rule the activity log uses.
 */
export function shortHash(hash: string): string {
  return hash.length > HASH_PREFIX ? `${hash.slice(0, HASH_PREFIX)}…` : hash;
}

/**
 * The first line of a message, marked when more follows, so nobody reads a
 * truncated error as the whole of it. The call's details hold the rest.
 */
export function firstLine(text: string): string {
  const trimmed = text.trim();
  const end = trimmed.indexOf("\n");
  return end === -1 ? trimmed : `${trimmed.slice(0, end).trimEnd()} …`;
}
