import {
  Empty,
  Enabled,
  ErrorBanner,
  Loading,
  Risk,
  Stale,
  Stat,
  State,
  Tainted,
} from "../components/common";
import { api } from "../sdk/client";
import { ago, tokens } from "../sdk/format";
import { useVisibleInterval } from "../sdk/live";
import { useAsync, useRefresh } from "../sdk/useAsync";
import type { Navigate } from "./route";

/** How often the audit chain's health is re-checked while the window is visible. */
const AUDIT_HEALTH_MS = 60_000;

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
  // The chain's health on its own, slower schedule: it changes only when
  // records are written, and the dashboard's poll should not pay for it.
  const health = useAsync(() => api.auditHealth(), []);
  useVisibleInterval(health.reload, AUDIT_HEALTH_MS);

  if (view.loading && !view.data) return <Loading what="the dashboard" />;

  const data = view.data;
  return (
    <>
      <div className="page-head">
        <h1>Dashboard</h1>
      </div>
      <p className="page-sub">Your agents, and what they are doing on this machine.</p>

      {view.error ? <ErrorBanner message={view.error} /> : null}
      {health.error ? (
        <ErrorBanner message={`The audit chain could not be checked: ${health.error}`} />
      ) : null}
      {health.data && !health.data.intact ? (
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
            {health.data ? (
              <>
                <Stat value={tokens(health.data.events)} label="audit events" />
                {/* An "intact" from a check that has since failed is not shown
                    as current: a reassurance nothing is confirming is the one
                    thing a failed check must not leave up. A "broken" stands. */}
                {!health.data.intact ? (
                  <Stat value="broken" label="audit chain" tone="danger" />
                ) : health.stale ? (
                  <Stat value="unknown" label="audit chain" tone="warn" />
                ) : (
                  <Stat value="intact" label="audit chain" tone="ok" />
                )}
              </>
            ) : health.error ? (
              <Stat value="unknown" label="audit chain" tone="warn" />
            ) : null}
          </div>
          {health.data ? (
            <p className="muted">
              The chain is checked in full once per launch, then as records are written; Settings
              rehashes all of it.{" "}
              {health.stale ? (
                <Stale since={health.data.checked_at} />
              ) : (
                <>Last checked {ago(health.data.checked_at)}.</>
              )}
            </p>
          ) : null}
        </>
      ) : null}
    </>
  );
}
