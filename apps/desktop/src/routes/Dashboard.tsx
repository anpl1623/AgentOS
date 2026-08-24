import {
  Empty,
  Enabled,
  ErrorBanner,
  Loading,
  Risk,
  Stat,
  State,
  Tainted,
} from "../components/common";
import { api } from "../sdk/client";
import { ago, tokens } from "../sdk/format";
import { useAsync, useRefresh } from "../sdk/useAsync";
import type { Navigate } from "./route";

/**
 * What is happening right now.
 *
 * Ordered by what would make someone act: things waiting on them, then things
 * running, then things that were refused. Counts last — a number is a reason to
 * look somewhere, not a thing to look at.
 */
export function Dashboard({ navigate }: { navigate: Navigate }) {
  const view = useAsync(() => api.dashboard(), []);
  useRefresh(view.reload);

  if (view.loading && !view.data) return <Loading what="the dashboard" />;

  const data = view.data;
  return (
    <>
      <div className="page-head">
        <h1>Dashboard</h1>
      </div>
      <p className="page-sub">Your agents, and what they are doing on this machine.</p>

      {view.error ? <ErrorBanner message={view.error} /> : null}
      {data && !data.audit_intact ? (
        <div className="banner error">
          The audit log does not verify. Its contents are unreliable from the first break onwards —
          see Settings.
        </div>
      ) : null}

      {data ? (
        <>
          {data.pending_approvals.length > 0 ? (
            <div className="banner warn">
              {data.pending_approvals.length === 1
                ? "An agent is waiting for your decision."
                : `${data.pending_approvals.length} agents are waiting for your decision.`}{" "}
              <button type="button" className="ghost" onClick={() => navigate({ name: "approvals" })}>
                Review →
              </button>
            </div>
          ) : null}

          <h2>Running</h2>
          <div className="panel">
            {data.running_tasks.length === 0 ? (
              <Empty>No agent is working right now.</Empty>
            ) : (
              data.running_tasks.map((task) => (
                <div
                  key={task.id}
                  className="row clickable"
                  onClick={() =>
                    navigate({ name: "tasks", ...(task.latest_run ? { runId: task.latest_run.id } : {}) })
                  }
                >
                  <div className="row-main">
                    <div className="row-title">{task.objective}</div>
                    <div className="row-meta">
                      <span>{task.agent_name}</span>
                      <span>started {ago(task.created_at)}</span>
                      {task.latest_run ? <span>{task.latest_run.steps} steps</span> : null}
                      {task.latest_run?.tainted ? <Tainted /> : null}
                    </div>
                  </div>
                  {task.latest_run ? <State state={task.latest_run.state} /> : null}
                </div>
              ))
            )}
          </div>

          <h2>Recently refused</h2>
          <div className="panel">
            {data.recent_refusals.length === 0 ? (
              <Empty>Nothing has been refused.</Empty>
            ) : (
              data.recent_refusals.map((execution) => (
                <div key={execution.id} className="row">
                  <div className="row-main">
                    <div className="row-title mono">{execution.tool}</div>
                    <div className="row-meta">
                      <span>{execution.error ?? execution.outcome}</span>
                    </div>
                  </div>
                  <Risk level={execution.risk} />
                  <span className="badge high">{execution.outcome.replace(/_/g, " ")}</span>
                </div>
              ))
            )}
          </div>

          <h2>Agents</h2>
          <div className="panel">
            {data.agents.length === 0 ? (
              <Empty>
                No agents yet.{" "}
                <button type="button" className="ghost" onClick={() => navigate({ name: "agents" })}>
                  Create one →
                </button>
              </Empty>
            ) : (
              data.agents.map((agent) => (
                <div
                  key={agent.id}
                  className="row clickable"
                  onClick={() => navigate({ name: "agents", agent: agent.name })}
                >
                  <div className="row-main">
                    <div className="row-title">{agent.name}</div>
                    <div className="row-meta">
                      <span>
                        {agent.provider}/{agent.model}
                      </span>
                      <span>{agent.tools.length} tools</span>
                    </div>
                  </div>
                  <Enabled status={agent.status} />
                </div>
              ))
            )}
          </div>

          <h2>At a glance</h2>
          <div className="grid stats">
            <Stat value={data.agents.length} label="agents" />
            <Stat value={data.running_tasks.length} label="running now" />
            <Stat
              value={data.pending_approvals.length}
              label="awaiting you"
              {...(data.pending_approvals.length > 0 ? { tone: "warn" as const } : {})}
            />
            <Stat value={tokens(data.audit_events)} label="audit events" />
            <Stat
              value={data.audit_intact ? "intact" : "broken"}
              label="audit chain"
              tone={data.audit_intact ? "ok" : "danger"}
            />
          </div>
        </>
      ) : null}
    </>
  );
}
