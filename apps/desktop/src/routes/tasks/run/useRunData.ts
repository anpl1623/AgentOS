import { useEffect, useMemo, useRef } from "react";

import type { AuditRecordView } from "../../../bindings/AuditRecordView";
import type { DelegationTreeView } from "../../../bindings/DelegationTreeView";
import type { RunSummary } from "../../../bindings/RunSummary";
import type { TraceView } from "../../../bindings/TraceView";
import { api, events } from "../../../sdk/client";
import { isLive } from "../../../sdk/format";
import { useLive, useVisibleInterval } from "../../../sdk/live";
import { type Async, useAsync } from "../../../sdk/useAsync";
import { useLoadedAt } from "../useLoadedAt";
import { treeRunIds } from "./delegationModel";
import {
  type Coalescer,
  LIVE_INTERVAL_MS,
  REFRESH_DEBOUNCE_MS,
  REFRESH_MAX_WAIT_MS,
  SETTLED_INTERVAL_MS,
  coalesce,
  concernsAnyRun,
} from "./refreshModel";

/** Everything the run screen reads, and the one way to read it again. */
export interface RunData {
  trace: Async<TraceView>;
  /** Every attempt at the run's task, oldest first; `null` until the task is known. */
  runs: Async<RunSummary[] | null>;
  audit: Async<AuditRecordView[]>;
  /** The runs delegated beneath this one; read only for a run that delegated. */
  tree: Async<DelegationTreeView | null>;
  /** When the trace last loaded, for the stale note. */
  loadedAt: number | null;
  /** Whether the run is still going, as the last trace said. */
  live: boolean;
  /** Whether the run asked any other agent for work. */
  delegating: boolean;
  /** One reload of everything per burst of events; `schedule` after acting on the run. */
  refresh: Coalescer;
}

/**
 * The run's trace, attempts, audit records and delegation tree, kept current.
 *
 * Not cached between visits, unlike the task list: whether the run is live is
 * what puts the Stop control on screen, and an answer from half a minute ago
 * must not be drawn as the present while the fresh one is on its way.
 */
export function useRunData(runId: string): RunData {
  const trace = useAsync(() => api.getTrace(runId), [runId]);
  const taskId = trace.data?.task_id ?? null;
  const runs = useAsync<RunSummary[] | null>(
    () => (taskId === null ? Promise.resolve(null) : api.listRuns(taskId)),
    [taskId],
  );
  const audit = useAsync(() => api.auditForRun(runId), [runId]);
  // The tree is read only for a run that delegated: every other run's tree
  // is the run alone, and the trace already says everything about it.
  const delegating = (trace.data?.delegations.length ?? 0) > 0;
  const tree = useAsync(
    () => (delegating ? api.delegationTree(runId) : Promise.resolve(null)),
    [runId, delegating],
  );
  const loadedAt = useLoadedAt(trace.data);

  const run = trace.data?.run ?? null;
  const live = run !== null && isLive(run.state);

  // Every run whose events change this screen: this one, and each run it
  // delegated to however far down, since a child's progress is drawn here.
  const watched = useMemo(() => {
    const ids = treeRunIds(tree.data);
    ids.add(runId);
    for (const delegation of trace.data?.delegations ?? []) {
      if (delegation.child_run_id !== null) ids.add(delegation.child_run_id);
    }
    return ids;
  }, [runId, tree.data, trace.data]);

  // One reload per burst of events for this run, and a slow timer behind it
  // for whatever the stream dropped. The timer reloads directly: through the
  // debounce, a run busy enough never to pause would hold it off as well.
  const reloads = useRef({
    trace: trace.reload,
    audit: audit.reload,
    runs: runs.reload,
    tree: tree.reload,
  });
  reloads.current = {
    trace: trace.reload,
    audit: audit.reload,
    runs: runs.reload,
    tree: tree.reload,
  };
  const refresh = useMemo(
    () =>
      coalesce(() => {
        reloads.current.trace();
        reloads.current.audit();
        reloads.current.runs();
        reloads.current.tree();
      }, REFRESH_DEBOUNCE_MS, REFRESH_MAX_WAIT_MS),
    [],
  );
  useEffect(() => () => refresh.cancel(), [refresh]);
  useLive(events.activity, (payload: unknown) => {
    if (concernsAnyRun(payload, watched)) refresh.schedule();
  });
  useVisibleInterval(refresh.flush, live ? LIVE_INTERVAL_MS : SETTLED_INTERVAL_MS);

  return { trace, runs, audit, tree, loadedAt, live, delegating, refresh };
}
