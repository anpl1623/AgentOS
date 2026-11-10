import { useMemo } from "react";

import type { DelegationTreeView } from "../../../bindings/DelegationTreeView";
import type { DelegationView } from "../../../bindings/DelegationView";
import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { StepView } from "../../../bindings/StepView";
import { EmptyState, ErrorBanner, Icon } from "../../../components/common";
import type { Navigate } from "../../route";
import { type DelegationPlacement, delegationRows } from "./delegationModel";
import { Delegations } from "./Delegations";
import { type Entry, type FailureAnchor, type TraceMode, entriesFor, marker } from "./runModel";
import { CallEntry, LiveEnd, StepEntry, UnplacedEntry } from "./TraceEntries";
import { allOpen, callIds, modeCounts, taintedKeys } from "./traceLook";
import type { TraceReading } from "./useRunControls";

const MODES: readonly { mode: TraceMode; label: string }[] = [
  { mode: "timeline", label: "Timeline" },
  { mode: "calls", label: "Tool calls" },
  { mode: "steps", label: "Steps" },
];

/**
 * What the run did, as a rail of steps and calls in the order they happened,
 * read three ways: everything, the calls alone, or the steps alone.
 */
export function Trace({
  entries,
  steps,
  executions,
  agent,
  failure,
  anchor,
  placement,
  tree,
  treeError,
  live,
  shown,
  reading,
  navigate,
}: {
  /** Every entry, merged, whatever the mode. */
  entries: readonly Entry[];
  steps: readonly StepView[];
  executions: readonly ExecutionView[];
  /** The run's own agent, the asker of its direct delegations. */
  agent: string;
  failure: string | null;
  anchor: FailureAnchor | null;
  placement: DelegationPlacement | null;
  tree: DelegationTreeView | null;
  /** Why the runs delegated beneath this one could not be read, for a run that delegated. */
  treeError: string | null;
  live: boolean;
  /** The run's state as shown, for the live end of the rail. */
  shown: string;
  reading: TraceReading;
  navigate: Navigate;
}) {
  const { mode, setMode, expanded, setAll } = reading;
  const visible = useMemo(() => entriesFor(mode, entries), [mode, entries]);
  const counts = useMemo(() => modeCounts(entries), [entries]);
  const tainted = useMemo(() => taintedKeys(entries), [entries]);
  const ids = useMemo(() => callIds(visible), [visible]);
  const everyOpen = allOpen(expanded, ids);
  const byId = useMemo(
    () => new Map(executions.map((execution) => [execution.id, execution])),
    [executions],
  );

  return (
    <section className="run-trace" aria-labelledby="run-trace">
      <div className="run-section-head">
        <h2 id="run-trace">What it did</h2>
      </div>
      <div className="toolbar run-trace-toolbar">
        <div className="segmented" role="group" aria-label="Show">
          {MODES.map((each) => (
            <button
              key={each.mode}
              type="button"
              aria-pressed={mode === each.mode}
              onClick={() => setMode(each.mode)}
            >
              {each.label}
              <span className="run-mode-count" aria-hidden="true">
                {counts[each.mode]}
              </span>
            </button>
          ))}
        </div>
        <span className="spacer" />
        {ids.length > 0 ? (
          <button type="button" className="ghost" onClick={() => setAll(ids, !everyOpen)}>
            <Icon name={everyOpen ? "chevron-right" : "chevron-down"} size={14} />
            {everyOpen ? "Close every call" : "Open every call"}
          </button>
        ) : null}
      </div>
      <div className="panel run-trace-panel">
        <TraceList
          mode={mode}
          entries={visible}
          steps={steps}
          byId={byId}
          tainted={tainted}
          expanded={expanded}
          anchor={anchor}
          failure={failure}
          reading={reading}
          rowsFor={(found) => delegationRows(agent, found, tree)}
          placement={placement}
          live={live}
          shown={shown}
          navigate={navigate}
        />
      </div>
      {treeError !== null ? (
        <ErrorBanner
          message={`The runs delegated beneath this one could not be read: ${treeError}`}
        />
      ) : null}
    </section>
  );
}

/**
 * The trace in the chosen mode.
 *
 * Steps are an ordered list; tool calls on their own are a plain list, since
 * the order of calls without the steps between them is not the story. The
 * steps list announces what is appended to it, and only that, so a live run
 * reads out each new step rather than the whole trace on every refresh. In
 * the Timeline mode a call can be expanded, and an expansion inside a live
 * region would be read out as if it had just happened; there a hidden line
 * announces the newest step instead.
 */
function TraceList({
  mode,
  entries,
  steps,
  byId,
  tainted,
  expanded,
  anchor,
  failure,
  reading,
  rowsFor,
  placement,
  live,
  shown,
  navigate,
}: {
  mode: TraceMode;
  entries: readonly Entry[];
  steps: readonly StepView[];
  byId: ReadonlyMap<string, ExecutionView>;
  tainted: ReadonlySet<string>;
  expanded: ReadonlySet<string>;
  anchor: FailureAnchor | null;
  failure: string | null;
  reading: TraceReading;
  /** The rows beneath a call, for the delegations it made. */
  rowsFor: (found: readonly DelegationView[]) => ReturnType<typeof delegationRows>;
  placement: DelegationPlacement | null;
  live: boolean;
  shown: string;
  navigate: Navigate;
}) {
  // Beneath the call that made them, or in the Steps mode, where calls are
  // not drawn, beneath the step that reports the call.
  const beneath = (executionId: string | null) => {
    const found = executionId === null ? undefined : placement?.byExecution.get(executionId);
    return found === undefined ? null : <Delegations rows={rowsFor(found)} navigate={navigate} />;
  };
  const unplaced = placement?.unplaced ?? [];
  if (entries.length === 0 && unplaced.length === 0) {
    return (
      <EmptyState
        icon={mode === "calls" ? "terminal" : "clock"}
        title={mode === "calls" ? "No tools were called." : "Nothing has been recorded yet."}
      >
        {live ? "Steps and calls appear here as the agent makes them." : undefined}
      </EmptyState>
    );
  }
  const rows = entries.map((entry) =>
    entry.kind === "step" ? (
      <StepEntry
        key={entry.key}
        step={entry.step}
        execution={
          entry.step.tool_execution_id === null
            ? null
            : (byId.get(entry.step.tool_execution_id) ?? null)
        }
        tainted={tainted.has(entry.key)}
        failure={anchor?.kind === "step" && anchor.ordinal === entry.step.ordinal ? failure : null}
        onReveal={reading.reveal}
      >
        {mode === "steps" ? beneath(entry.step.tool_execution_id) : null}
      </StepEntry>
    ) : (
      <CallEntry
        key={entry.key}
        execution={entry.execution}
        open={expanded.has(entry.execution.id)}
        tainted={tainted.has(entry.key)}
        failure={anchor?.kind === "execution" && anchor.id === entry.execution.id ? failure : null}
        onToggle={() => reading.toggle(entry.execution.id)}
      >
        {beneath(entry.execution.id)}
      </CallEntry>
    ),
  );
  // A call is recorded when it ends, so the delegations of one still in
  // progress have no row to sit under yet. They are drawn last, where that
  // call will appear, rather than left off a trace that is meant to say
  // what the run is doing.
  if (unplaced.length > 0) {
    rows.push(
      <UnplacedEntry key="delegations-unplaced" live={live}>
        <Delegations rows={rowsFor(unplaced)} navigate={navigate} />
      </UnplacedEntry>,
    );
  }
  // The rail runs on past the last entry while the run is live, ending in a
  // pulsing node that says what it is doing now. Hidden from assistive
  // technology, which hears the state from the header and each new step from
  // the line below; and outside the list's count of what happened.
  if (live) rows.push(<LiveEnd key="live-end" shown={shown} />);
  if (mode === "calls") {
    return (
      <ul className="trace" aria-labelledby="run-trace">
        {rows}
      </ul>
    );
  }
  const newest = steps.at(-1);
  return (
    <>
      <ol
        className="trace"
        aria-labelledby="run-trace"
        aria-live={mode === "steps" ? "polite" : undefined}
        aria-relevant={mode === "steps" ? "additions" : undefined}
      >
        {rows}
      </ol>
      {mode === "timeline" ? (
        <p className="visually-hidden" role="status">
          {newest ? `Step ${newest.ordinal}, ${marker(newest.kind).label}: ${newest.summary}` : ""}
        </p>
      ) : null}
    </>
  );
}
