import type { ApprovalView } from "../../bindings/ApprovalView";
import type { TaskSummary } from "../../bindings/TaskSummary";
import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { LiveDot, StatTile } from "../../components/common";
import type { Navigate } from "../route";
import { refusedTotal } from "./dashboardModel";
import {
  callsDelta,
  callsTotal,
  longestWait,
  runningBreakdown,
  runningHint,
  toolsCalled,
} from "./overviewModel";

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
 * The usage report has no per-day series, so the tool-call tile compares the
 * day with the window's daily average in words instead of drawing a
 * sparkline it would have to invent.
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
  days,
  navigate,
}: {
  now: number;
  approvals: readonly ApprovalView[];
  approvalsKnown: boolean;
  approvalsFailed: boolean;
  running: readonly TaskSummary[] | null;
  /** The last day's usage report. */
  today: readonly ToolUsageView[] | null;
  todayFailed: boolean;
  /** The tool panel's window, for the daily average. */
  usage: readonly ToolUsageView[] | null;
  days: number;
  navigate: Navigate;
}) {
  const waiting = approvalsKnown ? approvals.length : approvalsFailed ? UNKNOWN : null;
  const wait = approvalsKnown ? longestWait(approvals, now) : null;

  const breakdown = running ? runningBreakdown(running) : null;
  const runningHintText = breakdown ? runningHint(breakdown) : null;
  const live = (breakdown?.working ?? 0) > 0;

  const refused = today ? refusedTotal(today) : todayFailed ? UNKNOWN : null;

  const calls = today ? callsTotal(today) : null;
  const delta = calls !== null && usage ? callsDelta(calls, callsTotal(usage), days) : null;

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
      <div
        className={
          typeof refused === "number" && refused > 0
            ? "dash-kpi-refused active"
            : "dash-kpi-refused"
        }
      >
        <StatTile
          label="Refused · 24h"
          value={refused}
          icon="lock"
          hint="by policy, by you, or by budget"
          onClick={() => navigate({ name: "activity" })}
        />
      </div>
      <StatTile
        label="Tool calls · 24h"
        value={calls ?? (todayFailed ? UNKNOWN : null)}
        icon="zap"
        delta={delta ?? undefined}
        hint={today ? toolCount(toolsCalled(today)) : undefined}
      />
    </div>
  );
}

function toolCount(count: number): string {
  return count === 1 ? "across 1 tool" : `across ${count} tools`;
}
