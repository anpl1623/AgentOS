import { type ReactNode, useState } from "react";

import type { TaskNodeView } from "../../../bindings/TaskNodeView";
import {
  EmptyState,
  ErrorBanner,
  Icon,
  SkeletonRows,
  Status,
} from "../../../components/common";
import { useNow } from "../../../components/hooks";
import { api, describeError } from "../../../sdk/client";
import {
  type QueueRow,
  dependencyChoices,
  heldUntil,
  queueMark,
  queueRows,
  truncate,
} from "../tasksModel";
import { RowMarkIcon } from "./RowMark";

/** Work that has not started: what it waits for, and whether it ever will. */
export function QueuedTasks({
  graph,
  loading,
  error,
  schedulerNote: note,
  toSettings,
  onChanged,
  onCompose,
}: {
  graph: readonly TaskNodeView[] | null;
  loading: boolean;
  error: string | null;
  schedulerNote: string | null;
  toSettings: ReactNode;
  onChanged: () => void;
  /** Bring the person to the composer, to queue something. */
  onCompose: () => void;
}) {
  const now = useNow();
  const rows = graph ? queueRows(graph) : [];
  return (
    <section className="tasks-section" aria-labelledby="tasks-queued">
      <div className="tasks-section-head">
        <h2 id="tasks-queued">Queued</h2>
        {rows.length > 0 ? (
          <span className="tasks-count" aria-hidden="true">
            {rows.length}
          </span>
        ) : null}
      </div>
      {error ? <ErrorBanner message={`The queue could not be read: ${error}`} /> : null}
      {note !== null && rows.some((row) => row.state !== "unreachable") ? (
        <p className="banner warn tasks-scheduler-note">
          <Icon name="alert" size={14} />
          <span>
            {note} {toSettings}
          </span>
        </p>
      ) : null}
      <div className="panel">
        {loading && graph === null ? <SkeletonRows count={1} /> : null}
        {graph !== null && rows.length === 0 ? (
          <div className="tasks-empty-compact">
            <EmptyState
              icon="clock"
              title="Nothing is queued."
              action={
                <button type="button" className="ghost" onClick={onCompose}>
                  Queue a task
                </button>
              }
            />
          </div>
        ) : null}
        {rows.length > 0 ? (
          <div role="list" aria-labelledby="tasks-queued" className="stagger">
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
    </section>
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
      <div className={`row tasks-row tasks-queued queue-${row.state}`}>
        <RowMarkIcon mark={queueMark(row.state)} />
        <div className="row-main">
          <div className="row-title">{row.objective}</div>
          <div className="row-meta">
            <span className="tasks-agent">
              <Icon name="agents" size={12} />
              {row.agent}
            </span>
            {row.waitsFor.length > 0 ? (
              <span className="inline tasks-waits">
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
              <span className="tasks-culprit" title={row.culprit}>
                failed or cancelled: {truncate(row.culprit, 60)}
              </span>
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
          <Icon name="link" size={14} />
          Wait for…
        </button>
      </div>
      {editing ? (
        <div id={editorId} className="panel-body tasks-wait-editor">
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
