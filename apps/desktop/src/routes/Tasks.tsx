import { useCallback, useState } from "react";

import { ErrorBanner, Empty, Loading, Risk, State, Status, Tainted } from "../components/common";
import { api, describeError } from "../sdk/client";
import { ago, duration, isLive, tokens } from "../sdk/format";
import { useAsync, useRefresh } from "../sdk/useAsync";
import type { Navigate, Route } from "./route";

export function Tasks({ route, navigate }: { route: Route & { name: "tasks" }; navigate: Navigate }) {
  if (route.runId) {
    return <Trace runId={route.runId} navigate={navigate} />;
  }
  return <TaskList navigate={navigate} />;
}

function TaskList({ navigate }: { navigate: Navigate }) {
  const tasks = useAsync(() => api.listTasks(50), []);
  const agents = useAsync(() => api.listAgents(), []);
  useRefresh(tasks.reload);

  const [agentId, setAgentId] = useState("");
  const [objective, setObjective] = useState("");
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const enabled = agents.data?.filter((agent) => agent.status === "enabled") ?? [];
  const chosen = agentId || enabled[0]?.id || "";

  const start = useCallback(async () => {
    if (!chosen || objective.trim() === "") return;
    setStarting(true);
    setError(null);
    try {
      const started = await api.startTask(chosen, objective.trim());
      setObjective("");
      navigate({ name: "tasks", runId: started.run_id });
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setStarting(false);
    }
  }, [chosen, objective, navigate]);

  return (
    <>
      <div className="page-head">
        <h1>Tasks</h1>
      </div>
      <p className="page-sub">Give an agent an objective, and watch what it does about it.</p>

      <div className="panel">
        <div className="panel-body">
          {enabled.length === 0 ? (
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
                  placeholder="Review today's customer emails and draft replies to the routine ones."
                  onChange={(event) => setObjective(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void start();
                  }}
                />
              </div>
              <div className="inline">
                <select
                  value={chosen}
                  onChange={(event) => setAgentId(event.target.value)}
                  style={{ width: "auto", minWidth: 180 }}
                >
                  {enabled.map((agent) => (
                    <option key={agent.id} value={agent.id}>
                      {agent.name} · {agent.provider}/{agent.model}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="primary"
                  disabled={starting || objective.trim() === ""}
                  onClick={() => void start()}
                >
                  {starting ? "Starting…" : "Run"}
                </button>
              </div>
            </>
          )}
          {error ? (
            <div style={{ marginTop: 12 }}>
              <ErrorBanner message={error} />
            </div>
          ) : null}
        </div>
      </div>

      <h2>Recent</h2>
      {tasks.error ? <ErrorBanner message={tasks.error} /> : null}
      <div className="panel">
        {tasks.loading && !tasks.data ? <Loading what="tasks" /> : null}
        {tasks.data?.length === 0 ? <Empty>Nothing has been run yet.</Empty> : null}
        {tasks.data?.map((task) => (
          <div
            key={task.id}
            className="row clickable"
            onClick={() =>
              task.latest_run ? navigate({ name: "tasks", runId: task.latest_run.id }) : undefined
            }
          >
            <div className="row-main">
              <div className="row-title">{task.objective}</div>
              <div className="row-meta">
                <span>{task.agent_name}</span>
                <span>{ago(task.created_at)}</span>
                {task.latest_run ? (
                  <span>
                    {task.latest_run.steps} steps ·{" "}
                    {tokens(task.latest_run.input_tokens + task.latest_run.output_tokens)} tokens
                  </span>
                ) : null}
                {task.latest_run?.tainted ? <Tainted /> : null}
              </div>
            </div>
            <Status status={task.status} />
          </div>
        ))}
      </div>
    </>
  );
}

function Trace({ runId, navigate }: { runId: string; navigate: Navigate }) {
  const trace = useAsync(() => api.getTrace(runId), [runId]);
  const [cancelling, setCancelling] = useState(false);

  const live = trace.data ? isLive(trace.data.run.state) : false;
  // Only poll while there is something to see change.
  useRefresh(trace.reload, live ? 1500 : 60_000);

  const cancel = useCallback(async () => {
    setCancelling(true);
    try {
      await api.cancelRun(runId);
      trace.reload();
    } finally {
      setCancelling(false);
    }
  }, [runId, trace]);

  if (trace.loading && !trace.data) return <Loading what="the trace" />;
  if (trace.error) return <ErrorBanner message={trace.error} />;
  if (!trace.data) return null;

  const { run, steps, executions, approvals, objective, agent_name } = trace.data;

  return (
    <>
      <div className="page-head">
        <h1>{objective}</h1>
        <button type="button" className="ghost" onClick={() => navigate({ name: "tasks" })}>
          ← All tasks
        </button>
      </div>
      <p className="page-sub">
        {agent_name} · attempt {run.attempt} · started {ago(run.started_at)}
      </p>

      <div className="inline" style={{ marginBottom: 16 }}>
        <State state={run.state} />
        <span className="muted">{run.steps} steps</span>
        <span className="muted">
          {tokens(run.input_tokens)} in / {tokens(run.output_tokens)} out
        </span>
        {run.tainted ? <Tainted /> : null}
        <span className="right" />
        {live ? (
          <button type="button" className="danger" disabled={cancelling} onClick={() => void cancel()}>
            {cancelling ? "Stopping…" : "Stop this agent"}
          </button>
        ) : null}
      </div>

      {run.failure ? <div className="banner error">{run.failure}</div> : null}

      {run.result ? (
        <>
          <h2>Result</h2>
          <div className="panel">
            <div className="panel-body" style={{ whiteSpace: "pre-wrap" }}>
              {run.result}
            </div>
          </div>
        </>
      ) : null}

      {approvals.length > 0 ? (
        <>
          <h2>Approvals</h2>
          <div className="panel">
            {approvals.map((approval) => (
              <div key={approval.id} className="row">
                <div className="row-main">
                  <div className="row-title mono">{approval.tool}</div>
                  <div className="row-meta">
                    <span>{approval.reason}</span>
                  </div>
                </div>
                <Risk level={approval.risk} />
                <span className={`badge ${approval.status === "approved" ? "ok" : "high"}`}>
                  {approval.status}
                </span>
              </div>
            ))}
          </div>
        </>
      ) : null}

      <h2>What it did</h2>
      <div className="panel">
        {steps.length === 0 ? <Empty>Nothing recorded yet.</Empty> : null}
        {steps.map((step) => (
          <div key={step.ordinal} className="trace-step">
            <span className="trace-ordinal">{step.ordinal}</span>
            <span className="trace-marker">{marker(step.kind)}</span>
            <div className="row-main">
              <div>{step.summary}</div>
            </div>
          </div>
        ))}
      </div>

      <h2>Tool calls</h2>
      <div className="panel">
        {executions.length === 0 ? <Empty>No tools were called.</Empty> : null}
        {executions.map((execution) => (
          <div key={execution.id} className="row">
            <div className="row-main">
              <div className="row-title mono">{execution.tool}</div>
              <div className="row-meta">
                <span>{duration(execution.duration_ms)}</span>
                <span>decision: {execution.effect}</span>
                {execution.error ? <span className="faint">{execution.error}</span> : null}
              </div>
            </div>
            <Risk level={execution.risk} />
            <span className={`badge ${execution.executed ? "ok" : "high"}`}>
              {execution.outcome.replace(/_/g, " ")}
            </span>
          </div>
        ))}
      </div>
    </>
  );
}

function marker(kind: string): string {
  switch (kind) {
    case "toolcall":
    case "tool_call":
      return "▶";
    case "approval":
      return "?";
    case "verification":
      return "✓";
    case "recovery":
      return "↻";
    default:
      return "◆";
  }
}
