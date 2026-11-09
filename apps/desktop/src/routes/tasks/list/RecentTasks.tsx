import type { TaskSummary } from "../../../bindings/TaskSummary";
import {
  EmptyState,
  ErrorBanner,
  Icon,
  Row,
  SkeletonRows,
  Stale,
  Status,
  Tainted,
} from "../../../components/common";
import { ago, tokens } from "../../../sdk/format";
import type { Async } from "../../../sdk/useAsync";
import type { Navigate } from "../../route";
import {
  DEEP_LOAD,
  NO_FILTER,
  type TaskFilter,
  attemptNote,
  countNote,
  distinctOptions,
  emptyListMessage,
  filterTasks,
  isFiltering,
  showsStatusWithoutRun,
  statusChoices,
  statusLabel,
  taskMark,
  toggleStatus,
} from "../tasksModel";
import { RowMarkIcon } from "./RowMark";

/** The work that ran, newest first, narrowed by a toolbar. */
export function RecentTasks({
  tasks,
  limit,
  onDeepen,
  filter,
  onFilter,
  loadedAt,
  navigate,
  onCompose,
}: {
  tasks: Async<TaskSummary[]>;
  limit: number;
  /** Ask for {@link DEEP_LOAD} tasks rather than the first page. */
  onDeepen: () => void;
  filter: TaskFilter;
  onFilter: (filter: TaskFilter) => void;
  /** When the list last loaded, for the stale note. */
  loadedAt: number | null;
  navigate: Navigate;
  /** Bring the person to the composer, to give an agent its first task. */
  onCompose: () => void;
}) {
  const all = tasks.data ?? [];
  const shown = filterTasks(all, filter);
  const filtering = isFiltering(filter);
  const agentNames = distinctOptions(
    all.map((task) => task.agent_name),
    filter.agent,
  );
  const statuses = statusChoices(all, filter.status);
  const count = countNote(shown.length, all.length, filtering);

  return (
    <section className="tasks-section" aria-labelledby="tasks-recent">
      <div className="tasks-section-head">
        <h2 id="tasks-recent">Recent</h2>
        {all.length > 0 ? (
          <span className="tasks-count" aria-hidden="true">
            {all.length}
          </span>
        ) : null}
        <span className="spacer" />
        <button
          type="button"
          className="ghost"
          disabled={limit === DEEP_LOAD || tasks.loading || tasks.refreshing}
          onClick={onDeepen}
        >
          Load {DEEP_LOAD}
        </button>
      </div>
      <div className="toolbar tasks-toolbar">
        <div className="tasks-search">
          <Icon name="search" size={16} />
          <input
            type="search"
            aria-label="Filter by objective or agent"
            placeholder="Filter by objective or agent"
            value={filter.text}
            onChange={(event) => onFilter({ ...filter, text: event.target.value })}
          />
        </div>
        <select
          aria-label="Agent"
          value={filter.agent}
          onChange={(event) => onFilter({ ...filter, agent: event.target.value })}
        >
          <option value="">Every agent</option>
          {agentNames.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <div className="segmented tasks-status-filter" role="group" aria-label="Status">
          <button
            type="button"
            aria-pressed={filter.status === ""}
            onClick={() => onFilter({ ...filter, status: "" })}
          >
            All
          </button>
          {statuses.map((choice) => (
            <button
              key={choice.status}
              type="button"
              aria-pressed={filter.status === choice.status}
              onClick={() => onFilter(toggleStatus(filter, choice.status))}
            >
              <span
                className={`tasks-status-dot tone-${taskMark(choice.status).tone}`}
                aria-hidden="true"
              />
              {statusLabel(choice.status)}
              <span className="tasks-status-count">
                <span className="visually-hidden">, </span>
                {choice.count}
              </span>
            </button>
          ))}
        </div>
        <button
          type="button"
          className="ghost"
          disabled={!filtering}
          onClick={() => onFilter(NO_FILTER)}
        >
          <Icon name="x" size={14} />
          Clear
        </button>
        {count !== null ? <span className="faint tasks-count-note">{count}</span> : null}
      </div>

      {tasks.error ? <ErrorBanner message={`Tasks could not be read: ${tasks.error}`} /> : null}
      {tasks.stale && loadedAt !== null ? (
        <p className="muted">
          <Stale since={loadedAt} />
        </p>
      ) : null}
      <div className="panel">
        {tasks.loading && tasks.data === null ? <SkeletonRows count={3} /> : null}
        {tasks.data !== null && shown.length === 0 ? (
          <EmptyState
            icon={filtering ? "filter" : "tasks"}
            title={emptyListMessage(filtering)}
            action={
              filtering ? (
                <button type="button" onClick={() => onFilter(NO_FILTER)}>
                  Clear the filter
                </button>
              ) : (
                <button type="button" className="primary" onClick={onCompose}>
                  Write an objective
                </button>
              )
            }
          >
            {filtering
              ? "The filter applies to the tasks loaded so far."
              : "Give an agent an objective above, and its run appears here."}
          </EmptyState>
        ) : null}
        {shown.length > 0 ? (
          <div role="list" aria-labelledby="tasks-recent" className="stagger">
            {shown.map((task) => (
              <div role="listitem" key={task.id}>
                <TaskRow task={task} navigate={navigate} />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </section>
  );
}

/**
 * One task in Recent.
 *
 * A task with a run opens it. One without is plain text with `never ran`: a
 * row that is announced as a button and then does nothing is worse than a row
 * that is not a button.
 */
function TaskRow({ task, navigate }: { task: TaskSummary; navigate: Navigate }) {
  const run = task.latest_run;
  const attempt = attemptNote(task);
  return (
    <Row
      className={`tasks-row status-${taskMark(task.status).tone}`}
      onActivate={run ? () => navigate({ name: "tasks", runId: run.id }) : undefined}
    >
      <RowMarkIcon mark={taskMark(task.status)} />
      <div className="row-main">
        <div className="row-title">{task.objective}</div>
        <div className="row-meta">
          <span className="tasks-agent">
            <Icon name="agents" size={12} />
            {task.agent_name}
          </span>
          <span>{ago(task.created_at)}</span>
          {run ? (
            <span>
              {run.steps} steps · {tokens(run.input_tokens + run.output_tokens)} tokens
            </span>
          ) : null}
          {attempt !== null ? <span>{attempt}</span> : null}
          {run?.tainted ? <Tainted /> : null}
        </div>
      </div>
      {run ? (
        <Status status={task.status} />
      ) : (
        <>
          {showsStatusWithoutRun(task) ? <Status status={task.status} /> : null}
          <span className="verdict never">
            <span className="visually-hidden">task </span>Never ran
          </span>
        </>
      )}
      {run ? <Icon name="chevron-right" size={16} className="tasks-row-go" /> : null}
    </Row>
  );
}
