import { type ReactNode, useEffect, useRef, useState } from "react";

import type { ToolUsageView } from "../bindings/ToolUsageView";
import {
  Empty,
  Enabled,
  ErrorBanner,
  Risk,
  Row,
  SkeletonRows,
  Stale,
  Stat,
  State,
  Status,
  Tainted,
  Verdict,
} from "../components/common";
import { useNow } from "../components/hooks";
import { usePendingApprovals } from "../sdk/approvals";
import { cacheKey, useAsyncCached } from "../sdk/cache";
import { api } from "../sdk/client";
import { ago, duration, tokens } from "../sdk/format";
import { useVisibleInterval } from "../sdk/live";
import { useAsync } from "../sdk/useAsync";
import {
  NOT_YET,
  TOOL_WINDOWS,
  type ToolWindow,
  chainTile,
  failureRows,
  needsYou,
  readiness,
  refusalRows,
  refusedTotal,
  span,
  toolRows,
  unrecordedNote,
} from "./dashboardModel";
import type { Navigate } from "./route";

/**
 * How often the dashboard itself is re-read while visible.
 *
 * Slower than the 4s default because nothing on it waits on the poll to be
 * current: the screen also refreshes whenever it becomes visible, and the
 * Needs-you panel reads the approval store, which learns of requests by event.
 */
const DASHBOARD_MS = 8_000;

/**
 * How often the slower reads are repeated while visible: the audit chain's
 * health, tool usage, and setup. The chain is checked incrementally, so each
 * check costs only the records written since the last; usage is an aggregate
 * that a few seconds' staleness does not change the reading of.
 */
const SLOW_MS = 60_000;

/** Placeholder rows per list panel on first paint. */
const SKELETON_ROWS = 3;

/**
 * What is happening right now.
 *
 * Ordered by what would make someone act: things waiting on them, then things
 * running, then things that went wrong or were refused. Counts last — a number
 * is a reason to look somewhere, not a thing to look at.
 */
export function Dashboard({ navigate }: { navigate: Navigate }) {
  const now = useNow();
  const view = useAsyncCached(cacheKey("dashboard"), () => api.dashboard());
  useVisibleInterval(view.reload, DASHBOARD_MS);
  const pending = usePendingApprovals();

  // The chain's health on its own, slower schedule: it changes only when
  // records are written, and the dashboard's poll should not pay for it. Not
  // cached: an answer carried over from an earlier visit would be shown as
  // this visit's check.
  const health = useAsync(() => api.auditHealth(), []);
  useVisibleInterval(health.reload, SLOW_MS);

  const [days, setDays] = useState<ToolWindow>(7);
  const usage = useAsyncCached(cacheKey("tool_usage", days), () => api.toolUsage(days), [days]);
  const today = useAsyncCached(cacheKey("tool_usage", 1), () => api.toolUsage(1));
  useVisibleInterval(() => {
    usage.reload();
    today.reload();
  }, SLOW_MS);

  const data = view.data;
  const refusals = data ? refusalRows(data.recent_refusals) : [];

  // A new refusal arriving on the fast poll moves the day's count at once
  // rather than up to a minute later, beside a list that already shows it.
  // The first refusal seen is the one the day's count was just read with.
  const newestRefusal = refusals[0]?.id;
  const seenRefusal = useRef(newestRefusal);
  const reloadToday = today.reload;
  useEffect(() => {
    if (newestRefusal === undefined || newestRefusal === seenRefusal.current) return;
    if (seenRefusal.current !== undefined) reloadToday();
    seenRefusal.current = newestRefusal;
  }, [newestRefusal, reloadToday]);

  const settings = useAsyncCached(cacheKey("settings"), () => api.settings());
  const missing = readiness(settings.data, data?.agents ?? null);
  // Setup is re-read only while something is missing; once it is complete
  // there is nothing on this screen for a re-read to change.
  useVisibleInterval(() => {
    if (missing.length > 0) settings.reload();
  }, SLOW_MS);

  // Placeholders only before the first answer. A failed first read leaves the
  // panels empty beneath its error rather than shimmering forever.
  const first = data === null && view.loading;
  const chain = chainTile(health.data, health.stale, health.error !== null);
  const unrecorded = unrecordedNote(health.data);
  const queue = needsYou(pending.approvals);

  return (
    <>
      <div className="page-head">
        <h1>Dashboard</h1>
      </div>
      <p className="page-sub">Your agents, and what they are doing on this machine.</p>
      {first ? (
        <p className="visually-hidden" role="status">
          Loading the dashboard…
        </p>
      ) : null}

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
      {settings.error ? (
        <ErrorBanner
          message={`This installation's setup could not be checked: ${settings.error}`}
        />
      ) : null}

      {missing.length > 0 ? (
        <Panel id="dashboard-setup" title="Not set up yet">
          <List labelledBy="dashboard-setup">
            {missing.map((item) => (
              <Item key={item.id}>
                <Row onActivate={() => navigate(item.target)}>
                  <div className="row-main">
                    <div className="row-title">{item.problem}</div>
                    <div className="row-meta">
                      <span>{item.remedy}</span>
                    </div>
                  </div>
                  <span className="muted">
                    {item.target.name === "agents" ? "Agents" : "Settings"} →
                  </span>
                </Row>
              </Item>
            ))}
          </List>
        </Panel>
      ) : null}

      <Panel
        id="dashboard-needs-you"
        title="Needs you"
        above={
          pending.error ? (
            <ErrorBanner message={`Waiting approvals could not be read: ${pending.error}`} />
          ) : null
        }
      >
        {!pending.loaded ? (
          pending.error ? null : (
            <SkeletonRows count={1} />
          )
        ) : queue.shown.length === 0 ? (
          <Empty>Nothing is waiting for you.</Empty>
        ) : (
          <List labelledBy="dashboard-needs-you">
            {queue.shown.map((approval) => (
              <Item key={approval.id}>
                <Row onActivate={() => navigate({ name: "approvals", focus: approval.id })}>
                  <div className="row-main">
                    <div className="row-title mono">
                      {approval.agent_name} · {approval.tool}
                    </div>
                    <div className="row-meta">
                      <span>blocked for {span(approval.requested_at, now)}</span>
                      {approval.tainted ? <Tainted /> : null}
                    </div>
                  </div>
                  <Risk level={approval.risk} />
                </Row>
              </Item>
            ))}
            {queue.more > 0 ? (
              <Item>
                <Row onActivate={() => navigate({ name: "approvals" })}>
                  <div className="row-main">
                    <div className="row-title">
                      +{queue.more} more waiting
                      <span className="visually-hidden"> — open Approvals</span>
                    </div>
                  </div>
                </Row>
              </Item>
            ) : null}
          </List>
        )}
      </Panel>

      <Panel id="dashboard-running" title="Running">
        {first ? (
          <SkeletonRows count={SKELETON_ROWS} />
        ) : !data ? null : data.running_tasks.length === 0 ? (
          <Empty>No agent is working right now.</Empty>
        ) : (
          <List labelledBy="dashboard-running">
            {data.running_tasks.map((task) => {
              const run = task.latest_run;
              return (
                <Item key={task.id}>
                  <Row onActivate={run ? () => navigate({ name: "tasks", runId: run.id }) : undefined}>
                    <div className="row-main">
                      <div className="row-title">{task.objective}</div>
                      <div className="row-meta">
                        <span>{task.agent_name}</span>
                        {run ? (
                          <>
                            <span>running for {span(run.started_at, now)}</span>
                            <span>
                              {run.steps} steps · {tokens(run.input_tokens + run.output_tokens)}{" "}
                              tokens
                            </span>
                          </>
                        ) : (
                          <span>queued {ago(task.created_at)}</span>
                        )}
                        {run?.tainted ? <Tainted /> : null}
                      </div>
                    </div>
                    {run ? <State state={run.state} /> : <Status status={task.status} />}
                  </Row>
                </Item>
              );
            })}
          </List>
        )}
      </Panel>

      <Panel id="dashboard-failed" title="Failed recently">
        {first ? (
          <SkeletonRows count={1} />
        ) : !data ? null : data.recent_failures.length === 0 ? (
          <Empty>Nothing has failed recently.</Empty>
        ) : (
          <List labelledBy="dashboard-failed">
            {failureRows(data.recent_failures).map((failure) => {
              const runId = failure.runId;
              return (
                <Item key={failure.id}>
                  <Row onActivate={runId ? () => navigate({ name: "tasks", runId }) : undefined}>
                    <div className="row-main">
                      <div className="row-title">{failure.objective}</div>
                      <div className="row-meta">
                        <span>{failure.agent}</span>
                        <span title={failure.detail}>{failure.reason}</span>
                        <span>{ago(failure.failedAt)}</span>
                        {failure.tainted ? <Tainted /> : null}
                      </div>
                    </div>
                    <Status status="failed" />
                  </Row>
                </Item>
              );
            })}
          </List>
        )}
      </Panel>

      <Panel id="dashboard-refused" title="Recently refused">
        {first ? (
          <SkeletonRows count={SKELETON_ROWS} />
        ) : !data ? null : refusals.length === 0 ? (
          <Empty>Nothing has been refused.</Empty>
        ) : (
          <List labelledBy="dashboard-refused">
            {refusals.map((refusal) => (
              <Item key={refusal.id}>
                <Row onActivate={() => navigate({ name: "tasks", runId: refusal.runId })}>
                  <div className="row-main">
                    <div className="row-title mono">{refusal.tool}</div>
                    <div className="row-meta">
                      <span title={refusal.detail}>{refusal.reason}</span>
                      <span>{ago(refusal.startedAt)}</span>
                      {refusal.tainted ? <Tainted /> : null}
                    </div>
                  </div>
                  <Risk level={refusal.risk} />
                  <Verdict outcome={refusal.outcome} />
                </Row>
              </Item>
            ))}
          </List>
        )}
      </Panel>

      <Panel
        id="dashboard-tools"
        title="Tools"
        above={
          <>
            <div className="toolbar">
              <div className="segmented" role="group" aria-label="Tool usage over">
                {TOOL_WINDOWS.map((window) => (
                  <button
                    key={window}
                    type="button"
                    aria-pressed={days === window}
                    onClick={() => setDays(window)}
                  >
                    {window} days
                  </button>
                ))}
              </div>
            </div>
            {usage.error ? (
              <ErrorBanner message={`Tool usage could not be read: ${usage.error}`} />
            ) : null}
          </>
        }
      >
        {usage.data === null ? (
          usage.loading ? (
            <SkeletonRows count={SKELETON_ROWS} />
          ) : null
        ) : (
          <ToolList usage={usage.data} days={days} />
        )}
      </Panel>

      <Panel id="dashboard-agents" title="Agents">
        {first ? (
          <SkeletonRows count={SKELETON_ROWS} />
        ) : !data ? null : data.agents.length === 0 ? (
          <Empty>
            No agents yet.{" "}
            <button
              type="button"
              className="ghost"
              aria-label="Create an agent"
              onClick={() => navigate({ name: "agents" })}
            >
              Create one →
            </button>
          </Empty>
        ) : (
          <List labelledBy="dashboard-agents">
            {data.agents.map((agent) => (
              <Item key={agent.id}>
                <Row onActivate={() => navigate({ name: "agents", agent: agent.name })}>
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
                </Row>
              </Item>
            ))}
          </List>
        )}
      </Panel>

      <h2>At a glance</h2>
      {/* Every tile is drawn from the first paint, holding a dash until its
          answer arrives, so the grid keeps its shape rather than reflowing as
          each read lands. */}
      <div className="grid stats">
        <Stat value={data ? data.agents.length : NOT_YET} label="agents" />
        <Stat value={data ? data.running_tasks.length : NOT_YET} label="running now" />
        <Stat
          value={today.data ? refusedTotal(today.data) : today.error ? "unknown" : NOT_YET}
          label="refused (24h)"
        />
        <Stat value={health.data ? tokens(health.data.events) : NOT_YET} label="audit events" />
        <Stat value={chain.value} label={chain.label} tone={chain.tone} />
      </div>
      {unrecorded ? <p className="muted">{unrecorded}</p> : null}
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
  );
}

/**
 * A titled panel. The heading's id is what the list inside names itself by,
 * so a screen reader moving through lists hears "Running", not "list, 3 items".
 * `above` sits between the heading and the panel: controls, or an error.
 */
function Panel({
  id,
  title,
  above,
  children,
}: {
  id: string;
  title: string;
  above?: ReactNode;
  children: ReactNode;
}) {
  return (
    <>
      <h2 id={id}>{title}</h2>
      {above}
      <div className="panel">{children}</div>
    </>
  );
}

/**
 * A list of rows, named by its panel's heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`: the stylesheets have no reset for
 * a bare list, and wave 3 writes no CSS. What a screen reader hears is the
 * same.
 */
function List({ labelledBy, children }: { labelledBy: string; children: ReactNode }) {
  return (
    <div role="list" aria-labelledby={labelledBy}>
      {children}
    </div>
  );
}

/** One entry of a {@link List}. A `Row` that is a button cannot also be the item. */
function Item({ children }: { children: ReactNode }) {
  return <div role="listitem">{children}</div>;
}

/**
 * One row per tool called in the window.
 *
 * A tool refused more often than it ran carries a danger chip. Refusals are
 * otherwise drawn as the policy working, in the blocked tone; a tool that is
 * mostly refused is something else, an agent that keeps reaching for what it
 * may not have, and it is the row this panel exists to show.
 */
function ToolList({ usage, days }: { usage: readonly ToolUsageView[]; days: number }) {
  const rows = toolRows(usage);
  if (rows.length === 0) return <Empty>No tool has been called in the last {days} days.</Empty>;
  return (
    <List labelledBy="dashboard-tools">
      {rows.map((row) => (
        <Item key={row.tool}>
          <Row>
            <div className="row-main">
              <div className="row-title">
                <span className="mono">{row.tool}</span>{" "}
                {row.risk ? <Risk level={row.risk} /> : null}
              </div>
              <div className="row-meta">
                <span>
                  {row.calls === 1 ? "1 call" : `${row.calls} calls`} · {row.refused} refused ·{" "}
                  {duration(row.totalMs)}
                  {row.lastUsedAt ? ` · ${ago(row.lastUsedAt)}` : null}
                </span>
                {row.risk === null ? <span>no longer offered</span> : null}
              </div>
            </div>
            {row.mostlyRefused ? (
              <span className="verdict danger">
                <span className="visually-hidden">usage </span>Refused more than it ran
              </span>
            ) : null}
          </Row>
        </Item>
      ))}
    </List>
  );
}
