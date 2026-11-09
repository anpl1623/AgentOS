/**
 * What the dashboard decides, apart from how it draws it.
 *
 * Every rule that chooses which rows appear, in what order, and what a tile
 * claims lives here as a pure function, so a test fails when one of them
 * changes. The components in this folder only render what these return. What
 * stands between an installation and its first run is in `readinessModel.ts`,
 * and the overview's wording and arithmetic in `overviewModel.ts`.
 */

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { AuditHealth } from "../../bindings/AuditHealth";
import type { ExecutionView } from "../../bindings/ExecutionView";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { humanise } from "../../sdk/format";
import { sortApprovals } from "../approvals/approvalsModel";

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/** Milliseconds since the epoch, or `NaN` for a time that cannot be read. */
function timeOf(iso: string | null | undefined): number {
  return iso ? Date.parse(iso) : Number.NaN;
}

/**
 * How long ago `fromIso` was, as a bare length: `4m`, `2h`, `3d`.
 *
 * For "blocked for" and "running for", where `ago`'s trailing "ago" would read
 * wrongly. Whole units only, rounded down, so a run eleven minutes in never
 * claims twelve. A time that cannot be read, or lies in the future of a clock
 * that has drifted, says less than a minute rather than printing nonsense.
 */
export function span(fromIso: string, now: number): string {
  const elapsed = now - timeOf(fromIso);
  if (!Number.isFinite(elapsed) || elapsed < 60_000) return "less than a minute";
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/**
 * The first line of a message, marked when more follows.
 *
 * A row shows one line; the full text goes in its `title`. The ellipsis says
 * that the line on screen is not all there is, so nobody reads a truncated
 * error as the whole of it.
 */
export function firstLine(text: string): string {
  const trimmed = text.trim();
  const end = trimmed.indexOf("\n");
  return end === -1 ? trimmed : `${trimmed.slice(0, end).trimEnd()} …`;
}

// ---------------------------------------------------------------------------
// Needs you
// ---------------------------------------------------------------------------

/**
 * How many pending approvals the panel lists before folding the rest into one
 * row. Enough to see what kind of thing is waiting; the approvals screen is
 * where they are read.
 */
export const NEEDS_YOU_SHOWN = 5;

/** The approvals the panel lists, and how many it leaves to the approvals screen. */
export interface NeedsYou {
  shown: ApprovalView[];
  more: number;
}

/**
 * The pending approvals, most dangerous first, and the longest-waiting first
 * among equals.
 *
 * The approvals screen's own order, called rather than restated, so the five
 * listed here are the five at the top there and the two cannot drift apart. A
 * risk level this build does not know sorts as more dangerous than `critical`,
 * which is where an unknown should be when the question is what to look at
 * first.
 */
export function needsYou(
  approvals: readonly ApprovalView[],
  cap: number = NEEDS_YOU_SHOWN,
): NeedsYou {
  const sorted = sortApprovals(approvals);
  return { shown: sorted.slice(0, cap), more: Math.max(0, sorted.length - cap) };
}

/** Oldest first; a time that cannot be read goes last rather than first. */
function byTime(a: number, b: number): number {
  if (Number.isNaN(a)) return Number.isNaN(b) ? 0 : 1;
  if (Number.isNaN(b)) return -1;
  return a - b;
}

// ---------------------------------------------------------------------------
// Recently refused
// ---------------------------------------------------------------------------

/** One refused call, as its row shows it. */
export interface RefusalRow {
  id: string;
  tool: string;
  /** The run that made the call; the row opens its trace. */
  runId: string;
  risk: string;
  outcome: string;
  /** One line of why, for the row. */
  reason: string;
  /** All of it, for the row's `title`. */
  detail: string;
  startedAt: string;
  tainted: boolean;
}

/**
 * Refused calls, newest first.
 *
 * The reason is the runtime's error text where there is one, and the outcome
 * in words where there is not, so no row is left without a why.
 */
export function refusalRows(executions: readonly ExecutionView[]): RefusalRow[] {
  return [...executions]
    .sort((a, b) => byTime(timeOf(b.started_at), timeOf(a.started_at)))
    .map((execution) => {
      const detail = execution.error?.trim() || humanise(execution.outcome);
      return {
        id: execution.id,
        tool: execution.tool,
        runId: execution.run_id,
        risk: execution.risk,
        outcome: execution.outcome,
        reason: firstLine(detail),
        detail,
        startedAt: execution.started_at,
        tainted: execution.tainted,
      };
    });
}

// ---------------------------------------------------------------------------
// Failed recently
// ---------------------------------------------------------------------------

/** One failed task, as its row shows it. */
export interface FailureRow {
  id: string;
  objective: string;
  agent: string;
  reason: string;
  detail: string;
  failedAt: string;
  /** The failed attempt, or `null` for a task that never ran and has no trace. */
  runId: string | null;
  tainted: boolean;
}

/**
 * Said of a failed task with no run.
 *
 * The runtime fails a task without running it only when a scheduler found it
 * runnable and could not start it: the agent was disabled or gone, no provider
 * could be built for it, which is most often a missing key, or its policy no
 * longer compiles. A task abandoned because what it waits for failed is
 * cancelled, not failed, and is not listed here. The summary carries no
 * reason, so this names the candidates and where each is put right.
 */
export const NEVER_RAN =
  "Never ran: the scheduler could not start it. Check that its agent is enabled, has a valid policy, and that its provider has a key (Agents, Settings).";

/**
 * Failed tasks, most recent failure first.
 *
 * A failure is dated by when its run ended, falling back to the task's own
 * completion and then its creation, so a task that never ran still sorts by
 * something true about it.
 */
export function failureRows(tasks: readonly TaskSummary[]): FailureRow[] {
  return tasks
    .map((task) => {
      const run = task.latest_run;
      const detail =
        run?.failure?.trim() || (run ? "Failed without a reason recorded." : NEVER_RAN);
      return {
        id: task.id,
        objective: task.objective,
        agent: task.agent_name,
        reason: firstLine(detail),
        detail,
        failedAt: run?.completed_at ?? task.completed_at ?? task.created_at,
        runId: run?.id ?? null,
        tainted: run?.tainted ?? false,
      };
    })
    .sort((a, b) => byTime(timeOf(b.failedAt), timeOf(a.failedAt)));
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/** The windows the tools panel offers, in days. */
export const TOOL_WINDOWS = [7, 30] as const;

/** One of {@link TOOL_WINDOWS}. */
export type ToolWindow = (typeof TOOL_WINDOWS)[number];

/** One tool's calls over the window, as its row shows them. */
export interface ToolRow {
  tool: string;
  /** `null` for a tool with calls on record that the catalogue no longer offers. */
  risk: string | null;
  calls: number;
  executed: number;
  /** Calls that ran and failed: a part of `executed`, not beside it. */
  failed: number;
  /** Refused by the policy, by a person, or by the run's approval budget. */
  refused: number;
  totalMs: number;
  lastUsedAt: string | null;
  /** Refused more often than it ran: the row the panel exists to show. */
  mostlyRefused: boolean;
}

/** Every refusal, whoever made it: the policy, a person, or the approval budget. */
function refusedOf(usage: ToolUsageView): number {
  return usage.denied + usage.approval_denied;
}

/**
 * The tools that were called in the window, mostly-refused ones first, then
 * busiest first.
 *
 * The runtime lists every tool in the catalogue, called or not; a row of zeros
 * answers nothing, so those are left out. A tool refused more often than it
 * ran is an agent repeatedly reaching for something it is not allowed, which
 * is the pattern a prompt injection leaves, and it goes to the top rather than
 * wherever its call count would put it.
 */
export function toolRows(usage: readonly ToolUsageView[]): ToolRow[] {
  return usage
    .filter((each) => each.calls > 0)
    .map((each) => {
      const refused = refusedOf(each);
      return {
        tool: each.tool,
        risk: each.risk,
        calls: each.calls,
        executed: each.executed,
        failed: each.failed,
        refused,
        totalMs: each.total_duration_ms,
        lastUsedAt: each.last_used_at,
        mostlyRefused: refused > each.executed,
      };
    })
    .sort(
      (a, b) =>
        Number(b.mostlyRefused) - Number(a.mostlyRefused) ||
        b.calls - a.calls ||
        a.tool.localeCompare(b.tool),
    );
}

/**
 * Every refusal across every tool in a usage report.
 *
 * The dashboard asks for a one-day report for its `refused (24h)` tile. The
 * refusal list beside it is capped by the runtime, so counting that list
 * would stop at its cap and call it the day's total.
 */
export function refusedTotal(usage: readonly ToolUsageView[]): number {
  return usage.reduce((sum, each) => sum + refusedOf(each), 0);
}

// ---------------------------------------------------------------------------
// Tool meters
// ---------------------------------------------------------------------------

/** One part of a tool's meter. */
export interface MeterPart {
  kind: "ran" | "failed" | "refused" | "other";
  count: number;
}

/** A tool's meter: how long it is against the busiest tool, and what it is made of. */
export interface ToolMeter {
  /** Whole percent of the busiest tool's calls. */
  width: number;
  parts: MeterPart[];
}

/**
 * A tool's bar: its length against the busiest tool in the list, split by how
 * its calls ended.
 *
 * `failed` is a part of `executed`, so what ran cleanly is the difference.
 * Anything left over after the calls that ran and were refused, such as a call
 * with malformed arguments, is drawn as its own neutral part rather than
 * folded into a hue it does not belong to. Empty parts are left out, and a
 * tool with calls is never drawn narrower than a sliver.
 */
export function toolMeter(row: ToolRow, busiest: number): ToolMeter {
  if (!(busiest > 0) || !(row.calls > 0)) return { width: 0, parts: [] };
  const failed = Math.min(Math.max(0, row.failed), Math.max(0, row.executed));
  const ran = Math.max(0, row.executed - failed);
  const refused = Math.max(0, row.refused);
  const other = Math.max(0, row.calls - row.executed - refused);
  const parts: MeterPart[] = (
    [
      { kind: "ran", count: ran },
      { kind: "failed", count: failed },
      { kind: "refused", count: refused },
      { kind: "other", count: other },
    ] as const
  ).filter((part) => part.count > 0);
  return {
    width: Math.max(2, Math.min(100, Math.round((row.calls / busiest) * 100))),
    parts,
  };
}

// ---------------------------------------------------------------------------
// At a glance
// ---------------------------------------------------------------------------

/** What a `Stat` tile shows. */
export interface Tile {
  value: string;
  label: string;
  tone?: "ok" | "warn" | "danger";
}

/** What a tile shows before its first answer: a gap that keeps the grid's shape. */
export const NOT_YET = "—";

/**
 * The audit chain's tile.
 *
 * In order of what outranks what:
 *
 * - A break stands, even when the latest check failed: nothing can mend a
 *   chain, so an older "broken" is still true.
 * - Records this process failed to write stand the same way. The count only
 *   grows until the app restarts, and a chain that verifies is not "intact"
 *   when what happened is missing from it.
 * - An "intact" from a check that has since failed is not shown as current.
 *   A reassurance nothing is confirming is the one thing a failed check must
 *   not leave up.
 */
export function chainTile(health: AuditHealth | null, stale: boolean, failed: boolean): Tile {
  const label = "audit chain";
  if (health === null) {
    return failed ? { value: "unknown", label, tone: "warn" } : { value: NOT_YET, label };
  }
  if (!health.intact) return { value: "broken", label, tone: "danger" };
  if (health.unrecorded > 0) {
    return {
      value: "incomplete",
      label: `${label} · ${health.unrecorded} unwritten`,
      tone: "danger",
    };
  }
  if (stale) return { value: "unknown", label, tone: "warn" };
  return { value: "intact", label, tone: "ok" };
}

/**
 * The sentence beneath the tiles for records that were never written, or
 * `null` when there are none.
 */
export function unrecordedNote(health: AuditHealth | null): string | null {
  if (health === null || !(health.unrecorded > 0)) return null;
  const records = health.unrecorded === 1 ? "1 audit record" : `${health.unrecorded} audit records`;
  // "Verifies" only when it does: on a broken chain the banner above already
  // says it does not, and this sentence must not say the opposite under it.
  const chain = health.intact
    ? "The chain verifies, but it does not hold everything that happened"
    : "Besides the break, the log does not hold them";
  return `${records} could not be written since AgentOS was opened. ${chain}; check the disk the data directory is on.`;
}
