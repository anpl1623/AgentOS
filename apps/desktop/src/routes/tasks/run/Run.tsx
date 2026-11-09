import { useMemo, useState } from "react";

import { RunTimeline } from "../../../components/RunTimeline";
import { ErrorBanner, Loading, PageHeader, SkeletonRows } from "../../../components/common";
import { useNow } from "../../../components/hooks";
import type { Navigate } from "../../route";
import { placeDelegations } from "./delegationModel";
import { DelegatedBy } from "./Delegations";
import { LIVE_CLOCK_MS } from "./refreshModel";
import { AuditRecords } from "./AuditRecords";
import { RunApprovals } from "./RunApprovals";
import { Attempts, RunHeader, RunStats } from "./RunHeader";
import { canRetry, failureAnchor, mergeEntries, shownState } from "./runModel";
import { RunNotices } from "./RunNotices";
import { buildTimeline } from "./timelineModel";
import { Trace } from "./Trace";
import { modeCounts, pendingApprovals } from "./traceLook";
import { useRunActions, useTraceReading } from "./useRunControls";
import { useRunData } from "./useRunData";

/**
 * One attempt at a task, as evidence.
 *
 * The page an operator opens after something went wrong. It has to say what
 * happened in the order it happened, with the exact arguments of every call,
 * pinned to the audit records that prove it, and it must never claim more
 * than the runtime has said: a stop is `cancelling` until the run has ended,
 * and a failed refresh keeps the trace it had and says that it is old.
 *
 * This file only composes. Reading and refreshing the run is `useRunData`,
 * stopping, retrying and finding a row are `useRunControls`, and each section
 * is a module of its own.
 */
export function Run({
  runId,
  navigate,
  entering = false,
}: {
  runId: string;
  navigate: Navigate;
  /** Rise into place, as a view replacing another; fixed when first drawn. */
  entering?: boolean;
}) {
  const [rise] = useState(entering);
  const run = useRunData(runId);
  const { trace, runs, audit, tree, loadedAt, live, delegating, refresh } = run;
  const actions = useRunActions(runId, trace.data?.task_id ?? null, navigate, refresh);
  const reading = useTraceReading();

  // A live run's figures and chart run to the present; a finished one is fixed.
  const now = useNow(live ? LIVE_CLOCK_MS : 60_000);
  const data = trace.data;
  const entries = useMemo(
    () => (data ? mergeEntries(data.steps, data.executions) : []),
    [data],
  );
  const timeline = useMemo(() => (data ? buildTimeline(data, now) : null), [data, now]);
  const anchor = useMemo(
    () => (data ? failureAnchor(data.run, data.steps, data.executions) : null),
    [data],
  );
  const placement = useMemo(
    () => (data ? placeDelegations(data.executions, data.delegations) : null),
    [data],
  );

  if (data === null) {
    return (
      <div className="run-view">
        <PageHeader
          title="Run"
          parent={{ label: "Tasks", onActivate: () => navigate({ name: "tasks" }) }}
        />
        {trace.error ? (
          <ErrorBanner message={`The trace could not be read: ${trace.error}`} />
        ) : (
          <>
            <Loading what="the trace" />
            <SkeletonRows count={4} />
          </>
        )}
      </div>
    );
  }

  const shown = shownState(data.run.state, actions.cancelRequested);
  const attempts = runs.data ?? [];
  const latestId = runs.data?.at(-1)?.id ?? null;
  const retryable = canRetry(data.run, latestId, data.delegated_by !== null);

  return (
    <div className={rise ? "run-view screen-enter" : "run-view"}>
      <RunHeader
        trace={data}
        shown={shown}
        live={live}
        retryable={retryable}
        actions={actions}
        navigate={navigate}
      />
      {data.delegated_by ? (
        <DelegatedBy delegation={data.delegated_by} navigate={navigate} />
      ) : null}

      <RunStats
        trace={data}
        shown={shown}
        now={now}
        calls={modeCounts(entries).calls}
        attempts={attempts}
        stale={trace.stale ? loadedAt : null}
      />
      <Attempts run={data.run} runs={attempts} navigate={navigate} />

      <RunNotices
        trace={data}
        shown={shown}
        refreshError={trace.error}
        actionError={actions.error}
        pending={pendingApprovals(data.approvals)}
        anchor={anchor}
        onReveal={reading.reveal}
        navigate={navigate}
      />

      {timeline !== null ? (
        <section className="run-time" aria-labelledby="run-time">
          <div className="run-section-head">
            <h2 id="run-time">Where the time went</h2>
          </div>
          <RunTimeline timeline={timeline} onSelect={reading.reveal} />
        </section>
      ) : null}

      <Trace
        entries={entries}
        steps={data.steps}
        executions={data.executions}
        agent={data.agent_name}
        failure={data.run.failure}
        anchor={anchor}
        placement={placement}
        tree={tree.data}
        treeError={tree.error && delegating ? tree.error : null}
        live={live}
        shown={shown}
        reading={reading}
        navigate={navigate}
      />

      {data.approvals.length > 0 ? (
        <RunApprovals approvals={data.approvals} navigate={navigate} />
      ) : null}

      <AuditRecords
        records={audit.data}
        loading={audit.loading}
        error={audit.error}
        live={live}
      />
    </div>
  );
}
