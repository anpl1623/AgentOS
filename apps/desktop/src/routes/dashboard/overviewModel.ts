/**
 * The overview's wording and arithmetic: the greeting, the status line under
 * the title, the figures on the tiles, and the shapes the cards draw.
 *
 * Pure, like the rest of the dashboard's model, so the words a person reads
 * and the proportions a bar claims are fixed by a test rather than by how a
 * component happened to be written.
 */

import type { ApprovalView } from "../../bindings/ApprovalView";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import type { IconName } from "../../components/icons";
import type { LiveTone } from "../../components/LiveDot";
import type { StatDelta } from "../../components/StatTile";
import { isLive, tokens } from "../../sdk/format";
import { NOT_YET, type Tile, firstLine, span } from "./dashboardModel";

// ---------------------------------------------------------------------------
// Greeting
// ---------------------------------------------------------------------------

/**
 * The eyebrow over the title, by the hour on the person's own clock.
 *
 * The small hours say "Working late" rather than "Good morning": nobody at
 * their desk at 2am thinks of it as morning. An hour that cannot be read, which
 * a clock should never give, gets a plain hello rather than a wrong greeting.
 */
export function greeting(hour: number): string {
  if (!Number.isFinite(hour)) return "Hello";
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  if (h < 5) return "Working late";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

// ---------------------------------------------------------------------------
// Status line
// ---------------------------------------------------------------------------

/** The hue of one part of the status line. */
export type StatusTone = "ok" | "warn" | "danger" | "neutral";

/** One fact on the status line under the title. */
export interface StatusPart {
  id: "runtime" | "chain" | "setup";
  tone: StatusTone;
  /** An icon, or `live` for a pulsing dot. */
  mark: IconName | "live";
  text: string;
}

/** What the status line is drawn from. */
export interface StatusInput {
  /** Answers are coming from fixtures, not a runtime. */
  fixtures: boolean;
  /** The event feed's last read failed. */
  feedFailing: boolean;
  /** The audit chain's tile, from `chainTile`. */
  chain: Tile;
  /** How many setup checks are missing, or `null` before setup has been read. */
  missing: number | null;
}

/**
 * One line that says whether the system can be trusted right now: what the
 * window is connected to, whether the audit chain verifies, and whether setup
 * is finished.
 *
 * The runtime part reads the same facts as the sidebar's connection status,
 * in the same words for fixtures and for a runtime not answering, so the two
 * never tell a person different things. The chain part follows the
 * chain tile, which already decides what outranks what, and only rewords it:
 * a break or a gap stays in the danger hue, an unconfirmed answer in the
 * attention hue. Setup is left out until it has been read, so the line never
 * claims "complete" before it knows.
 */
export function statusLine(input: StatusInput): StatusPart[] {
  const parts: StatusPart[] = [];
  if (input.fixtures) {
    parts.push({
      id: "runtime",
      tone: "warn",
      mark: "alert",
      text: "Fixture data — not connected to a runtime",
    });
  } else if (input.feedFailing) {
    parts.push({ id: "runtime", tone: "warn", mark: "alert", text: "Runtime not answering" });
  } else {
    parts.push({ id: "runtime", tone: "ok", mark: "live", text: "Local runtime connected" });
  }

  parts.push({ id: "chain", mark: "shield", ...chainWords(input.chain) });

  if (input.missing !== null) {
    parts.push(
      input.missing === 0
        ? { id: "setup", tone: "ok", mark: "check", text: "Setup complete" }
        : {
            id: "setup",
            tone: "warn",
            mark: "settings",
            text: input.missing === 1 ? "1 setup step left" : `${input.missing} setup steps left`,
          },
    );
  }
  return parts;
}

function chainWords(chain: Tile): { tone: StatusTone; text: string } {
  switch (chain.value) {
    case "intact":
      return { tone: "ok", text: "Audit chain verified" };
    case "broken":
      return { tone: "danger", text: "Audit chain broken" };
    case "incomplete":
      return { tone: "danger", text: "Audit log incomplete" };
    case NOT_YET:
      return { tone: "neutral", text: "Checking the audit chain…" };
    default:
      return { tone: "warn", text: "Audit chain not confirmed" };
  }
}

/** The audit card's two lines: the chain's state, and what stands behind it. */
export interface AuditSummary {
  state: string;
  detail: string;
}

/**
 * The audit card's words, from the chain tile and the record count.
 *
 * The state is the tile's own word, capitalised to stand as a heading. The
 * detail counts the records, and carries on whatever the tile's label added
 * to its plain name, such as how many records went unwritten, so the card
 * drops nothing the tile said.
 */
export function auditSummary(chain: Tile, events: number | null): AuditSummary {
  const state = chain.value.charAt(0).toUpperCase() + chain.value.slice(1);
  const records = events === null ? "Not yet counted" : `${tokens(events)} records`;
  const extra = chain.label.split(" · ").slice(1);
  return { state, detail: [records, ...extra].join(" · ") };
}

// ---------------------------------------------------------------------------
// Tiles
// ---------------------------------------------------------------------------

/** Every call in a usage report, however it ended. */
export function callsTotal(usage: readonly ToolUsageView[]): number {
  return usage.reduce((sum, each) => sum + each.calls, 0);
}

/** How many tools in a usage report were called at all. */
export function toolsCalled(usage: readonly ToolUsageView[]): number {
  return usage.filter((each) => each.calls > 0).length;
}

/**
 * How far the last day's calls stand from the window's daily average, as a
 * tile's delta, or `null` when there is no average to compare with.
 *
 * Within 15% either way counts as level: a day's calls wander by that much on
 * their own, and an arrow for every wobble would teach a person to ignore it.
 * Neutral either way, because more tool calls is neither good nor bad news.
 */
export function callsDelta(today: number, windowCalls: number, days: number): StatDelta | null {
  if (!(days > 0) || !(windowCalls > 0)) return null;
  const average = windowCalls / days;
  const direction = today > average * 1.15 ? "up" : today < average * 0.85 ? "down" : "flat";
  const shown = average < 10 ? String(Math.round(average * 10) / 10) : String(Math.round(average));
  return { text: `vs ${shown}/day over ${days}d`, direction, sentiment: "neutral" };
}

/** The running tasks, split by whether anything is moving. */
export interface RunningBreakdown {
  /** Runs in a live state other than waiting on a person. */
  working: number;
  /** Runs paused on an approval. */
  waiting: number;
  /** Tasks with no run yet. */
  queued: number;
}

/** How many of the running tasks are working, waiting on a person, or not started. */
export function runningBreakdown(tasks: readonly TaskSummary[]): RunningBreakdown {
  const result: RunningBreakdown = { working: 0, waiting: 0, queued: 0 };
  for (const task of tasks) {
    const run = task.latest_run;
    if (run === null) result.queued += 1;
    else if (run.state === "waiting_for_approval") result.waiting += 1;
    else result.working += 1;
  }
  return result;
}

/** The running tile's hint: what the count is made of, or `null` when there is nothing. */
export function runningHint(breakdown: RunningBreakdown): string | null {
  const parts: string[] = [];
  if (breakdown.working > 0) parts.push(`${breakdown.working} working`);
  if (breakdown.waiting > 0) parts.push(`${breakdown.waiting} waiting on you`);
  if (breakdown.queued > 0) parts.push(`${breakdown.queued} queued`);
  return parts.length === 0 ? null : parts.join(" · ");
}

/**
 * How long the longest-waiting approval has been blocked, as a bare length,
 * or `null` when nothing is waiting or no request time can be read.
 */
export function longestWait(approvals: readonly ApprovalView[], now: number): string | null {
  let oldest: string | null = null;
  let oldestAt = Number.POSITIVE_INFINITY;
  for (const approval of approvals) {
    const at = Date.parse(approval.requested_at);
    if (Number.isFinite(at) && at < oldestAt) {
      oldestAt = at;
      oldest = approval.requested_at;
    }
  }
  return oldest === null ? null : span(oldest, now);
}

// ---------------------------------------------------------------------------
// Needs you
// ---------------------------------------------------------------------------

/**
 * How long an approval can wait before its card says so in the attention hue.
 * A quarter of an hour: long enough that a run has visibly stalled on it.
 */
export const LONG_WAIT_MS = 15 * 60_000;

/** Whether an approval has been waiting long enough to be called out. */
export function waitedLong(requestedAt: string, now: number): boolean {
  const at = Date.parse(requestedAt);
  return Number.isFinite(at) && now - at >= LONG_WAIT_MS;
}

/**
 * The one line a decision card says about what the agent wants to do.
 *
 * The runtime's explanation of the call where there is one, since that is the
 * action itself; the task's objective otherwise, so a card is never blank.
 */
export function decisionSummary(approval: ApprovalView): string {
  return firstLine(approval.explanation.trim() || approval.objective);
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/**
 * How long a run has been going, to the second: `45s`, `11m 04s`, `2h 05m`,
 * `3d 4h`.
 *
 * Seconds only while they matter to the reading. A time that cannot be read
 * shows a dash rather than a number that is not true, and a start in the
 * future of a drifted clock shows `0s`.
 */
export function runClock(fromIso: string, now: number): string {
  const started = Date.parse(fromIso);
  if (!Number.isFinite(started)) return "—";
  const seconds = Math.max(0, Math.floor((now - started) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${pad(seconds % 60)}s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${pad(minutes % 60)}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** How a running card shows its state. */
export interface RunMotion {
  tone: LiveTone;
  /** The dot pulses and the budget bar's sheen crosses: something is happening now. */
  moving: boolean;
}

/**
 * A running card's dot and sheen.
 *
 * A run paused on an approval is live but not moving: its dot holds still in
 * the attention hue, the same hue as the card waiting for the decision, and
 * its bar does not shimmer, since nothing is filling it. A state that is not
 * live at all, which a running list should not hold, is drawn still.
 */
export function runMotion(state: string): RunMotion {
  if (state === "waiting_for_approval") return { tone: "warn", moving: false };
  return { tone: "info", moving: isLive(state) };
}

/** How much of its step budget a run has used. */
export interface StepBudget {
  /** Whole percent, capped at 100. */
  percent: number;
  label: string;
}

/**
 * The share of an agent's step limit a run has spent, or `null` when the limit
 * is not known.
 *
 * The runtime gives no percentage for a run; its steps against the agent's
 * `max_steps` is the one honest measure of how far it can still go.
 */
export function stepBudget(steps: number, maxSteps: number | null | undefined): StepBudget | null {
  if (maxSteps === null || maxSteps === undefined || !(maxSteps > 0)) return null;
  const used = Math.max(0, steps);
  return {
    percent: Math.min(100, Math.round((used / maxSteps) * 100)),
    label: `${used} of ${maxSteps} steps`,
  };
}

/**
 * What an agent with work in the running list is doing: a run that is moving,
 * a run paused on a person, or a task that has not started.
 */
export type AgentActivity = "working" | "waiting" | "queued";

/** Which outranks which when one agent has several tasks in the list. */
const ACTIVITY_RANK: Readonly<Record<AgentActivity, number>> = {
  working: 3,
  waiting: 2,
  queued: 1,
};

/**
 * What each agent with work in the running list is doing, by agent id.
 *
 * Read the same way as the running cards, through {@link runMotion}, so an
 * agent is "working" with a pulsing dot only while a run of its own is moving:
 * an agent whose only run waits on a decision is still beside the card that
 * holds still for the same reason, and a task that has not started gives its
 * agent nothing to pulse for. One agent with several tasks shows the liveliest.
 */
export function agentActivity(
  running: readonly TaskSummary[],
): ReadonlyMap<string, AgentActivity> {
  const result = new Map<string, AgentActivity>();
  for (const task of running) {
    const run = task.latest_run;
    const activity: AgentActivity =
      run === null
        ? "queued"
        : run.state === "waiting_for_approval"
          ? "waiting"
          : runMotion(run.state).moving
            ? "working"
            : "queued";
    const before = result.get(task.agent_id);
    if (before === undefined || ACTIVITY_RANK[activity] > ACTIVITY_RANK[before]) {
      result.set(task.agent_id, activity);
    }
  }
  return result;
}
