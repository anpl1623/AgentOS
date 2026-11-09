import { type ReactNode, type RefObject, useState } from "react";

import type { TaskNodeView } from "../../../bindings/TaskNodeView";
import { EmptyState, ErrorBanner, Icon, Kbd, SkeletonRows } from "../../../components/common";
import { api, describeError } from "../../../sdk/client";
import { cacheKey, useAsyncCached } from "../../../sdk/cache";
import { useDraft } from "../../../sdk/drafts";
import type { Navigate } from "../../route";
import { dependencyChoices } from "../tasksModel";
import { isApplePlatform, submitHint, submitIntent, submitKeys, waitsLabel } from "./composerModel";
import { WaitForPicker } from "./WaitForPicker";

/** Whether this window runs on a Mac, read once: it does not change while the window is open. */
function useApple(): boolean {
  const [apple] = useState(() =>
    isApplePlatform(
      typeof navigator === "undefined" ? "" : navigator.platform || navigator.userAgent,
    ),
  );
  return apple;
}

/**
 * The new-task composer: an objective, the agent to give it to, and run it
 * now or queue it behind other tasks.
 *
 * The objective survives leaving the screen, but leaving is not guarded: an
 * objective is cheap to retype, and a confirmation on every trip to the
 * dashboard would be noise. It is a paragraph rather than a line, so Enter
 * starts a new line and Command-Enter (Control-Enter elsewhere) sends it.
 */
export function Composer({
  navigate,
  graph,
  schedulerNote: note,
  toSettings,
  onQueued,
  objectiveRef,
  composerRef,
}: {
  navigate: Navigate;
  graph: readonly TaskNodeView[];
  schedulerNote: string | null;
  toSettings: ReactNode;
  onQueued: () => void;
  /** The objective's field, so an empty list elsewhere can bring the person here. */
  objectiveRef: RefObject<HTMLTextAreaElement | null>;
  /** The composer itself, for when there is no objective to write in. */
  composerRef: RefObject<HTMLElement | null>;
}) {
  const agents = useAsyncCached(cacheKey("list_agents"), () => api.listAgents());
  const [objective, setObjective, clearObjective] = useDraft("task:objective", "");
  const [agentId, setAgentId] = useState("");
  const [dependsOn, setDependsOn] = useState<string[]>([]);
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState<"run" | "queue" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [queued, setQueued] = useState<string | null>(null);
  const apple = useApple();
  const keys = submitKeys(apple);

  const enabled = agents.data?.filter((agent) => agent.status === "enabled") ?? [];
  const chosen = agentId || enabled[0]?.id || "";
  const choices = dependencyChoices(graph);
  // Only choices still offered count: a dependency that finished since it was
  // picked is no longer in the list, and must not be sent invisibly.
  const waits = dependsOn.filter((id) => choices.some((choice) => choice.id === id));
  const ready = chosen !== "" && objective.trim() !== "" && busy === null;
  const queues = waits.length > 0;

  const run = async () => {
    if (!ready || queues) return;
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
      setPicking(false);
      setQueued(node.objective);
      onQueued();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(null);
    }
  };

  if (agents.loading && agents.data === null) {
    return (
      <div className="panel composer">
        <SkeletonRows count={2} />
      </div>
    );
  }

  return (
    <section ref={composerRef} className="panel composer" aria-labelledby="composer-title">
      <div className="composer-head">
        <span className="composer-mark" aria-hidden="true">
          <Icon name="sparkles" size={16} />
        </span>
        <h2 id="composer-title" className="composer-title">
          New task
        </h2>
      </div>
      {agents.error && agents.data === null ? (
        <ErrorBanner message={`Agents could not be read: ${agents.error}`} />
      ) : enabled.length === 0 ? (
        <EmptyState
          icon="agents"
          title="No enabled agent to give work to"
          action={
            <button type="button" className="primary" onClick={() => navigate({ name: "agents" })}>
              Create one
              <Icon name="chevron-right" size={16} />
            </button>
          }
        >
          A task is an objective given to an agent. Enable an agent, or create one, and it can
          take work from here.
        </EmptyState>
      ) : (
        <>
          <label htmlFor="objective" className="composer-label">
            Objective
          </label>
          <textarea
            id="objective"
            ref={objectiveRef}
            className="composer-objective"
            rows={3}
            value={objective}
            aria-describedby="objective-hint"
            aria-keyshortcuts={keys.aria}
            placeholder="Review today's customer emails and draft replies to the routine ones."
            onChange={(event) => setObjective(event.target.value)}
            onKeyDown={(event) => {
              const intent = submitIntent(
                {
                  key: event.key,
                  metaKey: event.metaKey,
                  ctrlKey: event.ctrlKey,
                  altKey: event.altKey,
                  isComposing: event.nativeEvent.isComposing,
                },
                waits.length,
              );
              if (intent === null) return;
              event.preventDefault();
              void (intent === "queue" ? queue() : run());
            }}
          />
          <p id="objective-hint" className="composer-hint">
            {submitHint(waits.length, apple)}
          </p>
          <div className="composer-bar">
            <div className="composer-agent">
              <label htmlFor="task-agent">
                <Icon name="agents" size={14} />
                Agent
              </label>
              <select
                id="task-agent"
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
              <button
                type="button"
                className={queues ? "ghost composer-waits active" : "ghost composer-waits"}
                aria-expanded={picking}
                aria-controls={picking ? "task-waits" : undefined}
                onClick={() => setPicking(!picking)}
              >
                <Icon name="link" size={14} />
                {waitsLabel(waits.length)}
                <Icon name="chevron-down" size={14} className="composer-waits-chevron" />
              </button>
            ) : null}
            <span className="spacer" />
            <div className="composer-actions">
              <button
                type="button"
                className={queues ? "primary" : undefined}
                disabled={!ready}
                aria-keyshortcuts={queues ? keys.aria : undefined}
                onClick={() => void queue()}
              >
                <Icon name="clock" size={16} />
                {busy === "queue" ? "Queueing…" : "Queue"}
                {queues ? <Kbd hidden>{keys.cap}</Kbd> : null}
              </button>
              <button
                type="button"
                className="primary"
                disabled={!ready || queues}
                aria-keyshortcuts={queues ? undefined : keys.aria}
                onClick={() => void run()}
              >
                <Icon name="play" size={16} />
                {busy === "run" ? "Starting…" : "Run"}
                {queues ? null : <Kbd hidden>{keys.cap}</Kbd>}
              </button>
            </div>
          </div>
          {picking && choices.length > 0 ? (
            <WaitForPicker
              id="task-waits"
              choices={choices}
              selected={waits}
              onChange={setDependsOn}
            />
          ) : null}
          {note !== null ? (
            <p className="composer-note">
              <Icon name="alert" size={14} />
              <span>
                {note} {toSettings}
              </span>
            </p>
          ) : null}
        </>
      )}
      <p className="visually-hidden" role="status">
        {queued !== null ? `Queued: ${queued}` : ""}
      </p>
      {queued !== null ? (
        <p className="composer-queued reveal">
          <Icon name="check" size={14} />
          Queued. It is listed under Queued below.
        </p>
      ) : null}
      {error ? (
        <div className="reveal">
          <ErrorBanner message={error} />
        </div>
      ) : null}
    </section>
  );
}
