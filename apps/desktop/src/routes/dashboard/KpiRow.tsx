import type { ApprovalView } from "../../bindings/ApprovalView";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { LiveDot, StatTile } from "../../components/common";
import type { Navigate } from "../route";
import { refusedTotal } from "./dashboardModel";
import {
  callsTotal,
  longestWait,
  runningBreakdown,
  runningHint,
  toolsCalled,
} from "./overviewModel";
import { callsDelta, dailyCalls, perDayCaption, perDayLabel } from "./usageModel";

/** A figure that failed to load, as opposed to one still loading (`null`). */
const UNKNOWN = "unknown";

/**
 * The four figures that say whether to look anywhere else: what waits on the
 * person, what is running, what was refused and how busy the tools were.
 *
 * Every tile is drawn from the first paint, holding a dash until its answer
 * arrives, so the row keeps its shape rather than reflowing as each read
 * lands. Only the figures that ask for a person take a loud tone: waiting
 * approvals in the attention hue, refusals in the blocked hue a refusal is
 * drawn in everywhere else.
 *
 * "Today" is the machine's own calendar day, since local midnight, as the
 * runtime counts it. The tool-call tile draws each day of the tool panel's
 * window beneath its figure, today last and still under way, and says what
 * the line is, since it carries no axis.
 */
export function KpiRow({
  now,
  approvals,
  approvalsKnown,
  approvalsFailed,
  running,
  today,
  todayFailed,
  usage,
  navigate,
}: {
  now: number;
  approvals: readonly ApprovalView[];
  approvalsKnown: boolean;
  approvalsFailed: boolean;
  running: readonly TaskSummary[] | null;
  /** Today's usage report, since local midnight. */
  today: readonly ToolUsageView[] | null;
  todayFailed: boolean;
  /** The tool panel's window, for the line of days. */
  usage: readonly ToolUsageView[] | null;
  navigate: Navigate;
}) {
  const waiting = approvalsKnown ? approvals.length : approvalsFailed ? UNKNOWN : null;
  const wait = approvalsKnown ? longestWait(approvals, now) : null;

  const breakdown = running ? runningBreakdown(running) : null;
  const runningHintText = breakdown ? runningHint(breakdown) : null;
  const live = (breakdown?.working ?? 0) > 0;

  const refused = today ? refusedTotal(today) : todayFailed ? UNKNOWN : null;

  const calls = today ? callsTotal(today) : null;
  const series = usage ? dailyCalls(usage) : null;
  const delta = calls !== null ? callsDelta(calls, series) : null;

  return (
    // Named as a group, as the "At a glance" heading once named these figures,
    // so a screen reader arriving at the first tile hears what the row is.
    <div className="grid dash-kpis stagger" role="group" aria-label="At a glance">
      <StatTile
        label="Waiting on you"
        value={waiting}
        icon="approvals"
        tone={typeof waiting === "number" && waiting > 0 ? "warn" : "accent"}
        hint={
          typeof waiting !== "number" ? undefined : wait ? `longest for ${wait}` : "nothing waiting"
        }
        onClick={() => navigate({ name: "approvals" })}
      />
      <StatTile
        label="Running now"
        value={running ? running.length : null}
        icon="play"
        tone={running && running.length > 0 ? "info" : "accent"}
        hint={
          runningHintText ? (
            <span className="dash-live-hint">
              <LiveDot tone="info" pulse={live} />
              {runningHintText}
            </span>
          ) : running ? (
            "no agent is working"
          ) : undefined
        }
        onClick={() => navigate({ name: "tasks" })}
      />
      <StatTile
        label="Refused · today"
        value={refused}
        icon="lock"
        tone={typeof refused === "number" && refused > 0 ? "blocked" : "accent"}
        hint="by policy, by you, or by budget"
        onClick={() => navigate({ name: "activity" })}
      />
      <StatTile
        label="Tool calls · today"
        value={calls ?? (todayFailed ? UNKNOWN : null)}
        icon="zap"
        delta={delta ?? undefined}
        hint={today ? toolCount(toolsCalled(today)) : undefined}
        trend={series ?? undefined}
        trendLabel={series ? perDayLabel("Tool calls", series.length) : undefined}
        trendCaption={series ? perDayCaption(series.length) : undefined}
      />
    </div>
  );
}

function toolCount(count: number): string {
  return count === 1 ? "across 1 tool" : `across ${count} tools`;
}
