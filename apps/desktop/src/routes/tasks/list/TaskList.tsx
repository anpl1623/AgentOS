import { useCallback, useRef, useState } from "react";

import { LiveDot, PageHeader } from "../../../components/common";
import { api } from "../../../sdk/client";
import { cacheKey, useAsyncCached } from "../../../sdk/cache";
import { useVisibleInterval } from "../../../sdk/live";
import type { Navigate } from "../../route";
import {
  DEEP_LOAD,
  NO_FILTER,
  SHALLOW_LOAD,
  type TaskFilter,
  runningCount,
  schedulerNote,
  toggleStatus,
} from "../tasksModel";
import { useLoadedAt } from "../useLoadedAt";
import { Composer } from "./Composer";
import { QueuedTasks } from "./QueuedTasks";
import { RecentTasks } from "./RecentTasks";

/** How often the list and the queue are re-read while visible. */
const LIST_MS = 4_000;

/** How often the scheduler's state is re-read; it changes only when someone flips it. */
const SCHEDULER_MS = 30_000;

/**
 * The task list: the composer that gives an agent work, the work still
 * waiting to start, and the work that ran.
 *
 * Read through the cache, so a return from a run shows the list at once and
 * refreshes it behind; the list is a summary, and every row it shows is
 * re-read within seconds.
 */
export function TaskList({
  navigate,
  entering = false,
}: {
  navigate: Navigate;
  /** Rise into place, as a view replacing the run; fixed when first drawn. */
  entering?: boolean;
}) {
  const [rise] = useState(entering);
  const [limit, setLimit] = useState(SHALLOW_LOAD);
  const tasks = useAsyncCached(cacheKey("list_tasks", limit), () => api.listTasks(limit), [limit]);
  const graph = useAsyncCached(cacheKey("task_graph"), () => api.taskGraph());
  const scheduler = useAsyncCached(cacheKey("scheduler_status"), () => api.schedulerStatus());
  useVisibleInterval(() => {
    tasks.reload();
    graph.reload();
  }, LIST_MS);
  useVisibleInterval(scheduler.reload, SCHEDULER_MS);

  const [filter, setFilter] = useState<TaskFilter>(NO_FILTER);
  const objective = useRef<HTMLTextAreaElement>(null);
  const composer = useRef<HTMLElement>(null);
  // To the objective, or, when there is none to write in because no agent is
  // enabled, to the composer and the action it offers instead: on a first
  // run, with no tasks and no agents, a button that went nowhere would be the
  // first thing the screen asked of the person.
  const compose = useCallback(() => {
    const field = objective.current;
    const target = field ?? composer.current;
    if (target === null) return;
    target.scrollIntoView({ block: "center" });
    (field ?? target.querySelector<HTMLElement>("button"))?.focus({ preventScroll: true });
  }, []);

  const reloadTasks = tasks.reload;
  const reloadGraph = graph.reload;
  const reloadWork = useCallback(() => {
    reloadTasks();
    reloadGraph();
  }, [reloadTasks, reloadGraph]);
  const tasksLoadedAt = useLoadedAt(tasks.data);
  const running = runningCount(tasks.data ?? []);

  const note = scheduler.error
    ? "Whether the scheduler is running could not be checked, so queued tasks may not start " +
      "on their own."
    : schedulerNote(scheduler.data);
  const toSettings = (
    <button type="button" className="ghost" onClick={() => navigate({ name: "settings" })}>
      Scheduler settings →
    </button>
  );

  return (
    <div className={rise ? "tasks-view screen-enter" : "tasks-view"}>
      <PageHeader
        eyebrow="Work"
        title="Tasks"
        subtitle="Give an agent an objective, and watch what it does about it."
        actions={
          running > 0 ? (
            <button
              type="button"
              className="ghost tasks-running"
              aria-pressed={filter.status === "running"}
              onClick={() => setFilter(toggleStatus(filter, "running"))}
            >
              <LiveDot tone="info" />
              {running} running
            </button>
          ) : null
        }
      />

      <Composer
        navigate={navigate}
        graph={graph.data ?? []}
        schedulerNote={note}
        toSettings={toSettings}
        onQueued={reloadWork}
        objectiveRef={objective}
        composerRef={composer}
      />

      <QueuedTasks
        graph={graph.data}
        loading={graph.loading}
        error={graph.error}
        schedulerNote={note}
        toSettings={toSettings}
        onChanged={graph.reload}
        onCompose={compose}
      />

      <RecentTasks
        tasks={tasks}
        limit={limit}
        onDeepen={() => setLimit(DEEP_LOAD)}
        filter={filter}
        onFilter={setFilter}
        loadedAt={tasksLoadedAt}
        navigate={navigate}
        onCompose={compose}
      />
    </div>
  );
}
