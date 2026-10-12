import { type ReactNode, useCallback, useMemo, useState } from "react";

import type { TaskNodeView } from "../bindings/TaskNodeView";
import type { TaskSummary } from "../bindings/TaskSummary";
import {
  Empty,
  ErrorBanner,
  PageHeader,
  Row,
  SkeletonRows,
  Stale,
  Status,
  Tainted,
} from "../components/common";
import { useNow } from "../components/hooks";
import { api, describeError } from "../sdk/client";
import { useDraft } from "../sdk/drafts";
import { ago, tokens } from "../sdk/format";
import { useVisibleInterval } from "../sdk/live";
import { useAsync } from "../sdk/useAsync";
import { Run, useLoadedAt } from "./Run";
import type { Navigate, Route } from "./route";
import {
  DEEP_LOAD,
  NO_FILTER,
  SHALLOW_LOAD,
  type QueueRow,
  type TaskFilter,
  attemptNote,
  countNote,
  dependencyChoices,
  distinctOptions,
  emptyListMessage,
  filterTasks,
  heldUntil,
  isFiltering,
  queueRows,
  schedulerNote,
  showsStatusWithoutRun,
  truncate,
} from "./tasksModel";

/** How often the list and the queue are re-read while visible. */
const LIST_MS = 4_000;

/** How often the scheduler's state is re-read; it changes only when someone flips it. */
const SCHEDULER_MS = 30_000;

/**
 * Tasks: the work given to agents, and one run of it.
 *
 * The run's trace is its own screen in its own file. It is the page an
 * operator opens after a failure, and it is read differently from the list.
 */
export function Tasks({
  route,
  navigate,
}: {
  route: Route & { name: "tasks" };
  navigate: Navigate;
}) {
  if (route.runId) {
    // Keyed so a different run starts from nothing: a stop requested on one
    // attempt must not show as `cancelling` on the next.
    return <Run key={route.runId} runId={route.runId} navigate={navigate} />;
  }
  return <TaskList navigate={navigate} />;
}

function TaskList({ navigate }: { navigate: Navigate }) {
  const [limit, setLimit] = useState(SHALLOW_LOAD);
  const tasks = useAsync(() => api.listTasks(limit), [limit]);
  const graph = useAsync(() => api.taskGraph(), []);
  const scheduler = useAsync(() => api.schedulerStatus(), []);
  useVisibleInterval(() => {
    tasks.reload();
    graph.reload();
  }, LIST_MS);
  useVisibleInterval(scheduler.reload, SCHEDULER_MS);

  const [filter, setFilter] = useState<TaskFilter>(NO_FILTER);
  const all = tasks.data ?? [];
  const shown = useMemo(() => filterTasks(all, filter), [all, filter]);
  const filtering = isFiltering(filter);
  const agentNames = distinctOptions(
    all.map((task) => task.agent_name),
    filter.agent,
  );
  const statuses = distinctOptions(
    all.map((task) => task.status),
    filter.status,
  );
  const count = countNote(shown.length, all.length, filtering);

  const reloadTasks = tasks.reload;
  const reloadGraph = graph.reload;
  const reloadWork = useCallback(() => {
    reloadTasks();
    reloadGraph();
  }, [reloadTasks, reloadGraph]);
  const tasksLoadedAt = useLoadedAt(tasks.data);

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
    <>
      <PageHeader
        title="Tasks"
        subtitle="Give an agent an objective, and watch what it does about it."
      />

      <NewTask
        navigate={navigate}
        graph={graph.data ?? []}
        schedulerNote={note}
        toSettings={toSettings}
        onQueued={reloadWork}
      />

      <Queued
        graph={graph.data}
        loading={graph.loading}
        error={graph.error}
        schedulerNote={note}
        toSettings={toSettings}
        onChanged={graph.reload}
      />

      <h2 id="tasks-recent">Recent</h2>
      <div className="toolbar">
        <input
          type="search"
          aria-label="Filter by objective or agent"
          placeholder="Filter by objective or agent"
          value={filter.text}
          onChange={(event) => setFilter({ ...filter, text: event.target.value })}
        />
        <select
          aria-label="Agent"
          value={filter.agent}
          onChange={(event) => setFilter({ ...filter, agent: event.target.value })}
        >
          <option value="">Every agent</option>
          {agentNames.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <select
          aria-label="Status"
          value={filter.status}
          onChange={(event) => setFilter({ ...filter, status: event.target.value })}
        >
          <option value="">Every status</option>
          {statuses.map((status) => (
            <option key={status} value={status}>
              {status}
            </option>
          ))}
        </select>
        <button type="button" disabled={!filtering} onClick={() => setFilter(NO_FILTER)}>
          Clear
        </button>
        <button
          type="button"
          disabled={limit === DEEP_LOAD || tasks.loading || tasks.refreshing}
          onClick={() => setLimit(DEEP_LOAD)}
        >
          Load {DEEP_LOAD}
        </button>
        {count !== null ? <span className="faint">{count}</span> : null}
      </div>

      {tasks.error ? <ErrorBanner message={`Tasks could not be read: ${tasks.error}`} /> : null}
      {tasks.stale && tasksLoadedAt !== null ? (
        <p className="muted">
          <Stale since={tasksLoadedAt} />
        </p>
      ) : null}
      <div className="panel">
        {tasks.loading ? <SkeletonRows count={3} /> : null}
        {tasks.data !== null && shown.length === 0 ? (
          <Empty>{emptyListMessage(filtering)}</Empty>
        ) : null}
        {shown.length > 0 ? (
          <div role="list" aria-labelledby="tasks-recent">
            {shown.map((task) => (
              <div role="listitem" key={task.id}>
                <TaskRow task={task} navigate={navigate} />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </>
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
    <Row onActivate={run ? () => navigate({ name: "tasks", runId: run.id }) : undefined}>
      <div className="row-main">
        <div className="row-title">{task.objective}</div>
        <div className="row-meta">
          <span>{task.agent_name}</span>
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
    </Row>
  );
}

/**
 * The new-task form: run now, or queue behind other tasks.
 *
 * The objective survives leaving the screen, but leaving is not guarded: a
 * one-line objective is cheap to retype, and a confirmation on every trip to
 * the dashboard would be noise.
 */
function NewTask({
  navigate,
  graph,
  schedulerNote: note,
  toSettings,
  onQueued,
}: {
  navigate: Navigate;
  graph: readonly TaskNodeView[];
  schedulerNote: string | null;
  toSettings: ReactNode;
  onQueued: () => void;
}) {
  const agents = useAsync(() => api.listAgents(), []);
  const [objective, setObjective, clearObjective] = useDraft("task:objective", "");
  const [agentId, setAgentId] = useState("");
  const [dependsOn, setDependsOn] = useState<string[]>([]);
  const [busy, setBusy] = useState<"run" | "queue" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [queued, setQueued] = useState<string | null>(null);

  const enabled = agents.data?.filter((agent) => agent.status === "enabled") ?? [];
  const chosen = agentId || enabled[0]?.id || "";
  const choices = dependencyChoices(graph);
  // Only choices still offered count: a dependency that finished since it was
  // picked is no longer in the list, and must not be sent invisibly.
  const waits = dependsOn.filter((id) => choices.some((choice) => choice.id === id));
  const ready = chosen !== "" && objective.trim() !== "" && busy === null;

  const run = async () => {
    if (!ready || waits.length > 0) return;
    setBusy("run");
    setError(null);
    setQueued(null);
    try {
      const started = await api.startTask(chosen, objective.trim());
      clearObjective();
      navigate({ name: "tasks", runId: started.run_id });
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(null);
    }
  };

  const queue = async () => {
    if (!ready) return;
    setBusy("queue");
    setError(null);
    setQueued(null);
    try {
      const node = await api.createTask({
        agent_id: chosen,
        objective: objective.trim(),
        depends_on: waits,
        scheduled_for: null,
      });
      clearObjective();
      setDependsOn([]);
      setQueued(node.objective);
      onQueued();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(null);
    }
  };

  if (agents.loading) {
    return (
      <div className="panel">
        <SkeletonRows count={1} />
      </div>
    );
  }

  return (
    <div className="panel">
      <div className="panel-body">
        {agents.error ? (
          <ErrorBanner message={`Agents could not be read: ${agents.error}`} />
        ) : enabled.length === 0 ? (
          <div className="muted">
            No enabled agent to give work to.{" "}
            <button type="button" className="ghost" onClick={() => navigate({ name: "agents" })}>
              Create one →
            </button>
          </div>
        ) : (
          <>
            <div className="field">
              <label htmlFor="objective">Objective</label>
              <input
                id="objective"
                value={objective}
                aria-describedby="objective-hint"
                placeholder="Review today's customer emails and draft replies to the routine ones."
                onChange={(event) => setObjective(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  event.preventDefault();
                  void (waits.length > 0 ? queue() : run());
                }}
              />
              <p id="objective-hint" className="field-note">
                {waits.length > 0
                  ? "Press Enter to queue it behind the tasks it waits for."
                  : "Press Enter to run it now."}
              </p>
            </div>
            <div className="field">
              <label htmlFor="task-agent">Agent</label>
              <select
                id="task-agent"
                className="select-compact"
                value={chosen}
                onChange={(event) => setAgentId(event.target.value)}
              >
                {enabled.map((agent) => (
                  <option key={agent.id} value={agent.id}>
                    {agent.name} · {agent.provider}/{agent.model}
                  </option>
                ))}
              </select>
            </div>
            {choices.length > 0 ? (
              <div className="field">
                <label htmlFor="task-waits">Wait for</label>
                <select
                  id="task-waits"
                  multiple
                  // Chrome draws a multiple select of one row as a drop-down
                  // reading "0 selected", which hides that several may be
                  // chosen; two rows keep it a list box.
                  size={Math.max(2, Math.min(4, choices.length))}
                  aria-describedby="task-waits-note"
                  value={waits}
                  onChange={(event) =>
                    setDependsOn(Array.from(event.target.selectedOptions, (option) => option.value))
                  }
                >
                  {choices.map((choice) => (
                    <option key={choice.id} value={choice.id}>
                      {choice.label}
                    </option>
                  ))}
                </select>
                <p id="task-waits-note" className="field-note">
                  Optional. A task that waits for others is queued, and starts once they all
                  succeed.
                </p>
              </div>
            ) : null}
            <div className="inline">
              <button
                type="button"
                className="primary"
                disabled={!ready || waits.length > 0}
                onClick={() => void run()}
              >
                {busy === "run" ? "Starting…" : "Run"}
              </button>
              <button type="button" disabled={!ready} onClick={() => void queue()}>
                {busy === "queue" ? "Queueing…" : "Queue"}
              </button>
            </div>
            {note !== null ? (
              <p className="field-note">
                {note} {toSettings}
              </p>
            ) : null}
          </>
        )}
        <p className="visually-hidden" role="status">
          {queued !== null ? `Queued: ${queued}` : ""}
        </p>
        {queued !== null ? (
          <p className="field-note">Queued. It is listed under Queued below.</p>
        ) : null}
        {error ? (
          <div className="reveal">
            <ErrorBanner message={error} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** Work that has not started: what it waits for, and whether it ever will. */
function Queued({
  graph,
  loading,
  error,
  schedulerNote: note,
  toSettings,
  onChanged,
}: {
  graph: readonly TaskNodeView[] | null;
  loading: boolean;
  error: string | null;
  schedulerNote: string | null;
  toSettings: ReactNode;
  onChanged: () => void;
}) {
  const now = useNow();
  const rows = graph ? queueRows(graph) : [];
  return (
    <>
      <h2 id="tasks-queued">Queued</h2>
      {error ? <ErrorBanner message={`The queue could not be read: ${error}`} /> : null}
      {note !== null && rows.some((row) => row.state !== "unreachable") ? (
        <p className="muted">
          {note} {toSettings}
        </p>
      ) : null}
      <div className="panel">
        {loading ? <SkeletonRows count={1} /> : null}
        {graph !== null && rows.length === 0 ? <Empty>Nothing is queued.</Empty> : null}
        {rows.length > 0 ? (
          <div role="list" aria-labelledby="tasks-queued">
            {rows.map((row) => (
              <div role="listitem" key={row.id}>
                <QueuedRow
                  row={row}
                  node={graph?.find((node) => node.id === row.id) ?? null}
                  graph={graph ?? []}
                  now={now}
                  onChanged={onChanged}
                />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </>
  );
}

/**
 * One queued task, with "Wait for…" to make it wait for another.
 *
 * A refused edge shows the runtime's message as it was written: a cycle is
 * refused with its path, and that path is the explanation.
 */
function QueuedRow({
  row,
  node,
  graph,
  now,
  onChanged,
}: {
  row: QueueRow;
  node: TaskNodeView | null;
  graph: readonly TaskNodeView[];
  now: number;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [choice, setChoice] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choices = node ? dependencyChoices(graph, node) : [];
  const editorId = `queued-wait-${row.id}`;

  const add = async () => {
    if (choice === "") return;
    setSaving(true);
    setError(null);
    try {
      await api.addTaskDependency(row.id, choice);
      setEditing(false);
      setChoice("");
      onChanged();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <div className="row">
        <div className="row-main">
          <div className="row-title">{row.objective}</div>
          <div className="row-meta">
            <span>{row.agent}</span>
            {row.waitsFor.length > 0 ? (
              <span className="inline">
                <span>waits for {row.waitsFor.length}</span>
                {row.waitsFor.map((blocker) => (
                  <span key={blocker.id} className="tag" title={blocker.full}>
                    {blocker.label}
                  </span>
                ))}
              </span>
            ) : null}
            {row.heldUntil !== null ? (
              <span title={row.heldUntil}>{heldUntil(row.heldUntil, now)}</span>
            ) : null}
            {row.culprit !== null ? (
              <span title={row.culprit}>failed or cancelled: {truncate(row.culprit, 60)}</span>
            ) : null}
          </div>
        </div>
        {row.state === "ready" ? (
          <span className="verdict live">
            <span className="visually-hidden">queue </span>Ready
          </span>
        ) : row.state === "unreachable" ? (
          <span className="verdict danger">
            <span className="visually-hidden">queue </span>Blocked by a failed task
          </span>
        ) : (
          <Status status={node?.status ?? "pending"} />
        )}
        <button
          type="button"
          className="ghost"
          aria-expanded={editing}
          aria-controls={editing ? editorId : undefined}
          disabled={choices.length === 0}
          title={
            choices.length === 0 ? "There is no other incomplete task to wait for." : undefined
          }
          onClick={() => {
            setEditing(!editing);
            setError(null);
          }}
        >
          Wait for…
        </button>
      </div>
      {editing ? (
        <div id={editorId} className="panel-body">
          <div className="inline">
            <select
              className="select-compact"
              aria-label={`Task for “${truncate(row.objective, 40)}” to wait for`}
              value={choice}
              onChange={(event) => setChoice(event.target.value)}
            >
              <option value="">Choose a task…</option>
              {choices.map((each) => (
                <option key={each.id} value={each.id}>
                  {each.label}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="primary"
              disabled={choice === "" || saving}
              onClick={() => void add()}
            >
              {saving ? "Adding…" : "Add"}
            </button>
            <button type="button" className="ghost" onClick={() => setEditing(false)}>
              Cancel
            </button>
          </div>
          {error ? (
            <div className="reveal">
              <ErrorBanner message={error} />
            </div>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
