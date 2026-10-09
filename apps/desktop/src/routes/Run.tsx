import { useEffect, useMemo, useRef, useState } from "react";

import type { ApprovalView } from "../bindings/ApprovalView";
import type { AuditRecordView } from "../bindings/AuditRecordView";
import type { ExecutionView } from "../bindings/ExecutionView";
import type { RunSummary } from "../bindings/RunSummary";
import type { StepView } from "../bindings/StepView";
import { ArgumentBlock, Visible } from "../components/ArgumentBlock";
import { RunTimeline } from "../components/RunTimeline";
import {
  Empty,
  ErrorBanner,
  Loading,
  PageHeader,
  Risk,
  Row,
  SkeletonRows,
  Stale,
  State,
  Tainted,
  Verdict,
} from "../components/common";
import { useCopy, useNow } from "../components/hooks";
import { api, describeError, events } from "../sdk/client";
import { ago, clock, duration, humanise, isLive, tokens } from "../sdk/format";
import { useLive, useVisibleInterval } from "../sdk/live";
import { useAsync } from "../sdk/useAsync";
import { shortHash } from "./activityModel";
import { firstLine } from "./dashboardModel";
import type { Navigate } from "./route";
import {
  type Entry,
  type FailureAnchor,
  LIVE_INTERVAL_MS,
  REFRESH_DEBOUNCE_MS,
  REFRESH_MAX_WAIT_MS,
  SETTLED_INTERVAL_MS,
  type SpanTarget,
  type TraceMode,
  approvalTone,
  attemptsLabel,
  buildTimeline,
  canRetry,
  coalesce,
  concernsRun,
  entriesFor,
  failureAnchor,
  marker,
  mergeEntries,
  shownState,
  transcript,
} from "./runModel";

/** How often a live run's timeline extends to the present when nothing else changes. */
const LIVE_CLOCK_MS = 5_000;

const MODES: readonly { mode: TraceMode; label: string }[] = [
  { mode: "timeline", label: "Timeline" },
  { mode: "calls", label: "Tool calls" },
  { mode: "steps", label: "Steps" },
];

/** The element id of a row on this screen, for scrolling to it. */
function rowId(target: SpanTarget): string {
  switch (target.kind) {
    case "step":
      return `run-step-${target.ordinal}`;
    case "execution":
      return `run-execution-${target.id}`;
    case "approval":
      return `run-approval-${target.id}`;
  }
}

/**
 * When `data` last changed to something, as the moment a load succeeded.
 *
 * `useAsync` keeps the last good answer through a failed reload; this is how
 * old that answer is, for the `Stale` note beside it.
 */
export function useLoadedAt(data: unknown): number | null {
  const [at, setAt] = useState<number | null>(null);
  useEffect(() => {
    if (data !== null) setAt(Date.now());
  }, [data]);
  return at;
}

/**
 * One attempt at a task, as evidence.
 *
 * The page an operator opens after something went wrong. It has to say what
 * happened in the order it happened, with the exact arguments of every call,
 * pinned to the audit records that prove it, and it must never claim more
 * than the runtime has said: a stop is `cancelling` until the run has ended,
 * and a failed refresh keeps the trace it had and says that it is old.
 */
export function Run({ runId, navigate }: { runId: string; navigate: Navigate }) {
  const trace = useAsync(() => api.getTrace(runId), [runId]);
  const taskId = trace.data?.task_id ?? null;
  const runs = useAsync<RunSummary[] | null>(
    () => (taskId === null ? Promise.resolve(null) : api.listRuns(taskId)),
    [taskId],
  );
  const audit = useAsync(() => api.auditForRun(runId), [runId]);
  const loadedAt = useLoadedAt(trace.data);

  const run = trace.data?.run ?? null;
  const live = run !== null && isLive(run.state);

  // One reload per burst of events for this run, and a slow timer behind it
  // for whatever the stream dropped. The timer reloads directly: through the
  // debounce, a run busy enough never to pause would hold it off as well.
  const reloads = useRef({ trace: trace.reload, audit: audit.reload, runs: runs.reload });
  reloads.current = { trace: trace.reload, audit: audit.reload, runs: runs.reload };
  const refresh = useMemo(
    () =>
      coalesce(() => {
        reloads.current.trace();
        reloads.current.audit();
        reloads.current.runs();
      }, REFRESH_DEBOUNCE_MS, REFRESH_MAX_WAIT_MS),
    [],
  );
  useEffect(() => () => refresh.cancel(), [refresh]);
  useLive(events.activity, (payload: unknown) => {
    if (concernsRun(payload, runId)) refresh.schedule();
  });
  useVisibleInterval(refresh.flush, live ? LIVE_INTERVAL_MS : SETTLED_INTERVAL_MS);

  const [cancelRequested, setCancelRequested] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const { copy, state: copyState } = useCopy();

  const [mode, setMode] = useState<TraceMode>("timeline");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [scrollTo, setScrollTo] = useState<{ id: string; nonce: number } | null>(null);

  // A live run's chart runs to the present; a finished one is fixed.
  const now = useNow(live ? LIVE_CLOCK_MS : 60_000);
  const data = trace.data;
  const entries = useMemo(
    () => (data ? mergeEntries(data.steps, data.executions) : []),
    [data],
  );
  const timeline = useMemo(() => (data ? buildTimeline(data, now) : null), [data, now]);
  const anchor = useMemo(
    () => (data ? failureAnchor(data.run, data.steps, data.executions) : null),
    [data],
  );

  // Scroll after the render that put the row on screen: switching mode or
  // expanding a call happens in the same update as the request.
  useEffect(() => {
    if (scrollTo === null) return;
    const row = document.getElementById(scrollTo.id);
    row?.scrollIntoView({ block: "center" });
    row?.focus({ preventScroll: true });
  }, [scrollTo]);

  const reveal = (target: SpanTarget | FailureAnchor) => {
    if (target.kind === "execution") {
      setExpanded((open) => new Set(open).add(target.id));
      if (mode === "steps") setMode("timeline");
    } else if (target.kind === "step" && mode === "calls") {
      setMode("timeline");
    }
    setScrollTo((previous) => ({ id: rowId(target), nonce: (previous?.nonce ?? 0) + 1 }));
  };

  const toggle = (id: string) =>
    setExpanded((open) => {
      const next = new Set(open);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const cancel = async () => {
    setCancelRequested(true);
    setActionError(null);
    try {
      const stopping = await api.cancelRun(runId);
      // Nothing in this process was running it; the next read says what is.
      if (!stopping) setCancelRequested(false);
    } catch (failure) {
      setCancelRequested(false);
      setActionError(`The run could not be stopped: ${describeError(failure)}`);
    }
    refresh.schedule();
  };

  const retry = async () => {
    if (taskId === null) return;
    setRetrying(true);
    setActionError(null);
    try {
      const started = await api.retryTask(taskId);
      navigate({ name: "tasks", runId: started.run_id });
    } catch (failure) {
      setActionError(`The task could not be retried: ${describeError(failure)}`);
      setRetrying(false);
    }
  };

  const parent = { label: "Tasks", onActivate: () => navigate({ name: "tasks" }) };

  if (data === null) {
    return (
      <>
        <PageHeader title="Run" parent={parent} />
        {trace.error ? (
          <ErrorBanner message={`The trace could not be read: ${trace.error}`} />
        ) : (
          <>
            <Loading what="the trace" />
            <SkeletonRows count={4} />
          </>
        )}
      </>
    );
  }

  const shown = shownState(data.run.state, cancelRequested);
  const attempts = runs.data ?? [];
  const latestId = runs.data?.at(-1)?.id ?? null;
  const retryable = canRetry(data.run, latestId);
  const visible = entriesFor(mode, entries);

  return (
    <>
      <PageHeader
        title={data.objective}
        parent={parent}
        subtitle={
          <>
            {data.agent_name} · attempt {data.run.attempt} · started {ago(data.run.started_at)}
            {data.run.completed_at ? ` · ended ${ago(data.run.completed_at)}` : null}
          </>
        }
        actions={<Attempts run={data.run} runs={attempts} navigate={navigate} />}
      />

      <div className="toolbar">
        {shown === "cancelling" ? (
          <span className="verdict cancelling">
            <span className="visually-hidden">run </span>Cancelling
          </span>
        ) : (
          <State state={data.run.state} />
        )}
        <span className="muted">{data.run.steps} steps</span>
        <span className="muted">
          {tokens(data.run.input_tokens)} in / {tokens(data.run.output_tokens)} out
        </span>
        {data.run.tainted ? <Tainted /> : null}
        {trace.stale && loadedAt !== null ? <Stale since={loadedAt} /> : null}
        <span className="spacer" />
        <button type="button" onClick={() => void copy(transcript(data, shown))}>
          {copyState === "copied"
            ? "Copied"
            : copyState === "failed"
              ? "Could not copy"
              : "Copy this run"}
        </button>
        <span className="visually-hidden" role="status">
          {copyState === "copied"
            ? "The run was copied as text."
            : copyState === "failed"
              ? "The run could not be copied."
              : ""}
        </span>
        {live ? (
          <button
            type="button"
            className="danger"
            disabled={cancelRequested}
            onClick={() => void cancel()}
          >
            {cancelRequested ? "Stopping…" : "Stop this agent"}
          </button>
        ) : retryable ? (
          <button
            type="button"
            className="primary"
            disabled={retrying || taskId === null}
            onClick={() => void retry()}
          >
            {retrying ? "Retrying…" : "Retry"}
          </button>
        ) : null}
      </div>

      {shown === "cancelling" ? (
        <p className="muted" role="status">
          Asked to stop. The run has not ended yet: a tool call already in progress finishes or is
          abandoned before it does.
        </p>
      ) : null}
      {trace.error ? (
        <ErrorBanner message={`The trace could not be refreshed: ${trace.error}`} />
      ) : null}
      {actionError ? <ErrorBanner message={actionError} /> : null}
      {data.run.tainted && data.run.taint_sources.length > 0 ? (
        <p className="muted">
          Untrusted data came from{" "}
          {data.run.taint_sources.map((source, index) => (
            <span key={source}>
              {index > 0 ? ", " : null}
              <span className="mono">
                <Visible text={source} />
              </span>
            </span>
          ))}
          .
        </p>
      ) : null}

      {data.run.failure ? (
        <div className="banner error">
          <span className="prose">
            <Visible text={data.run.failure} />
          </span>
          {anchor !== null ? (
            <>
              {" "}
              <button type="button" className="ghost" onClick={() => reveal(anchor)}>
                Show where it failed →
              </button>
            </>
          ) : null}
        </div>
      ) : null}

      {data.run.result ? (
        <>
          <h2>Result</h2>
          <div className="panel">
            <div className="panel-body prose">
              <Visible text={data.run.result} />
            </div>
          </div>
        </>
      ) : null}

      {timeline !== null ? (
        <>
          <h2>Where the time went</h2>
          <RunTimeline timeline={timeline} onSelect={reveal} />
        </>
      ) : null}

      <h2 id="run-trace">What it did</h2>
      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Show">
          {MODES.map((each) => (
            <button
              key={each.mode}
              type="button"
              aria-pressed={mode === each.mode}
              onClick={() => setMode(each.mode)}
            >
              {each.label}
            </button>
          ))}
        </div>
      </div>
      <div className="panel">
        <TraceList
          mode={mode}
          entries={visible}
          steps={data.steps}
          expanded={expanded}
          anchor={anchor}
          failure={data.run.failure}
          onToggle={toggle}
          onReveal={reveal}
        />
      </div>

      {data.approvals.length > 0 ? (
        <Approvals approvals={data.approvals} navigate={navigate} />
      ) : null}

      <AuditRecords
        records={audit.data}
        loading={audit.loading}
        error={audit.error}
        live={live}
      />
    </>
  );
}

/** The attempt switcher: every attempt at the task, each opening its own run. */
function Attempts({
  run,
  runs,
  navigate,
}: {
  run: RunSummary;
  runs: readonly RunSummary[];
  navigate: Navigate;
}) {
  const label = attemptsLabel(run, runs);
  if (label === null) return null;
  return (
    <div className="inline" role="group" aria-label="Attempts">
      <span className="muted">{label}</span>
      {runs.map((attempt) => (
        <button
          key={attempt.id}
          type="button"
          aria-pressed={attempt.id === run.id}
          onClick={() => {
            if (attempt.id !== run.id) navigate({ name: "tasks", runId: attempt.id });
          }}
        >
          <span className="visually-hidden">attempt </span>
          {attempt.attempt} <State state={attempt.state} />
        </button>
      ))}
    </div>
  );
}

/**
 * The trace in the chosen mode.
 *
 * Steps are an ordered list; tool calls on their own are a plain list, since
 * the order of calls without the steps between them is not the story. The
 * steps list announces what is appended to it, and only that, so a live run
 * reads out each new step rather than the whole trace on every refresh. In
 * the Timeline mode a call can be expanded, and an expansion inside a live
 * region would be read out as if it had just happened; there a hidden line
 * announces the newest step instead.
 */
function TraceList({
  mode,
  entries,
  steps,
  expanded,
  anchor,
  failure,
  onToggle,
  onReveal,
}: {
  mode: TraceMode;
  entries: readonly Entry[];
  steps: readonly StepView[];
  expanded: ReadonlySet<string>;
  anchor: FailureAnchor | null;
  failure: string | null;
  onToggle: (id: string) => void;
  onReveal: (target: SpanTarget) => void;
}) {
  if (entries.length === 0) {
    return (
      <Empty>
        {mode === "calls" ? "No tools were called." : "Nothing has been recorded yet."}
      </Empty>
    );
  }
  const rows = entries.map((entry) =>
    entry.kind === "step" ? (
      <StepRow
        key={entry.key}
        step={entry.step}
        failure={anchor?.kind === "step" && anchor.ordinal === entry.step.ordinal ? failure : null}
        onReveal={onReveal}
      />
    ) : (
      <ExecutionRow
        key={entry.key}
        execution={entry.execution}
        open={expanded.has(entry.execution.id)}
        failure={anchor?.kind === "execution" && anchor.id === entry.execution.id ? failure : null}
        onToggle={() => onToggle(entry.execution.id)}
      />
    ),
  );
  if (mode === "calls") {
    return (
      <ul className="plain-list" aria-labelledby="run-trace">
        {rows}
      </ul>
    );
  }
  const newest = steps.at(-1);
  return (
    <>
      <ol
        className="plain-list"
        aria-labelledby="run-trace"
        aria-live={mode === "steps" ? "polite" : undefined}
        aria-relevant={mode === "steps" ? "additions" : undefined}
      >
        {rows}
      </ol>
      {mode === "timeline" ? (
        <p className="visually-hidden" role="status">
          {newest ? `Step ${newest.ordinal}, ${marker(newest.kind).label}: ${newest.summary}` : ""}
        </p>
      ) : null}
    </>
  );
}

/** One step: when, which, what kind, and what the runtime wrote about it. */
function StepRow({
  step,
  failure,
  onReveal,
}: {
  step: StepView;
  failure: string | null;
  onReveal: (target: SpanTarget) => void;
}) {
  const { glyph, label } = marker(step.kind);
  const executionId = step.tool_execution_id;
  return (
    <li id={`run-step-${step.ordinal}`} className="trace-step" tabIndex={-1}>
      <span className="event-time">{clock(step.at)}</span>
      <span className="trace-ordinal">{step.ordinal}</span>
      <span className="trace-marker" aria-hidden="true">
        {glyph}
      </span>
      <div className="row-main">
        <div className="prose">
          <span className="visually-hidden">{label}: </span>
          <Visible text={step.summary} />
        </div>
        {failure !== null ? (
          <div className="banner error" role="note">
            The run failed here: <Visible text={failure} />
          </div>
        ) : null}
      </div>
      {executionId !== null ? (
        <button
          type="button"
          className="ghost"
          onClick={() => onReveal({ kind: "execution", id: executionId })}
        >
          Show call
        </button>
      ) : null}
    </li>
  );
}

/**
 * One tool call. The row opens its details: the exact arguments, the
 * decision, the outcome, the duration, the call id and the error in full.
 *
 * The arguments may fold long strings, since the call already happened and
 * nothing here can change what it did.
 */
function ExecutionRow({
  execution,
  open,
  failure,
  onToggle,
}: {
  execution: ExecutionView;
  open: boolean;
  failure: string | null;
  onToggle: () => void;
}) {
  const detailsId = `run-execution-${execution.id}-details`;
  return (
    <li id={`run-execution-${execution.id}`} tabIndex={-1}>
      <button
        type="button"
        className="row clickable"
        aria-expanded={open}
        aria-controls={open ? detailsId : undefined}
        onClick={onToggle}
      >
        <span className="event-time">{clock(execution.started_at)}</span>
        <div className="row-main">
          <div className="row-title mono">
            <span className="visually-hidden">tool call: </span>
            {execution.tool}
          </div>
          <div className="row-meta">
            <span>{duration(execution.duration_ms)}</span>
            <span>decision: {execution.effect}</span>
            {execution.tainted ? <Tainted label="after untrusted data" /> : null}
            {execution.error ? (
              <span className="muted">
                <Visible text={firstLine(execution.error)} />
              </span>
            ) : null}
          </div>
        </div>
        <Risk level={execution.risk} />
        <Verdict outcome={execution.outcome} />
      </button>
      {failure !== null ? (
        <div className="panel-body">
          <div className="banner error" role="note">
            The run failed here: <Visible text={failure} />
          </div>
        </div>
      ) : null}
      {open ? (
        <div id={detailsId} className="panel-body">
          <ArgumentBlock json={execution.arguments} id={detailsId} decided />
          <dl className="facts flush">
            <dt>Decision</dt>
            <dd>{execution.effect}</dd>
            <dt>Outcome</dt>
            <dd>
              {humanise(execution.outcome)}
              {execution.executed ? "" : " — it did not execute"}
            </dd>
            <dt>Duration</dt>
            <dd>{duration(execution.duration_ms)}</dd>
            <dt>Started</dt>
            <dd title={execution.started_at}>{clock(execution.started_at)}</dd>
            <dt>Call id</dt>
            <dd className="mono">{execution.call_id}</dd>
            {execution.approval_id !== null ? (
              <>
                <dt>Approval</dt>
                <dd className="mono">{execution.approval_id}</dd>
              </>
            ) : null}
            <dt>Run tainted</dt>
            <dd>{execution.tainted ? "yes, when this call was made" : "no"}</dd>
            {execution.error ? (
              <>
                <dt>Error</dt>
                <dd className="prose muted">
                  <Visible text={execution.error} />
                </dd>
              </>
            ) : null}
          </dl>
        </div>
      ) : null}
    </li>
  );
}

/** Approvals the run asked for. A pending one opens its card. */
function Approvals({
  approvals,
  navigate,
}: {
  approvals: readonly ApprovalView[];
  navigate: Navigate;
}) {
  return (
    <>
      <h2 id="run-approvals">Approvals</h2>
      <div className="panel">
        <div role="list" aria-labelledby="run-approvals">
          {approvals.map((approval) => {
            const status = humanise(approval.status);
            return (
              <div
                role="listitem"
                key={approval.id}
                id={`run-approval-${approval.id}`}
                tabIndex={-1}
              >
                <Row
                  onActivate={
                    approval.status === "pending"
                      ? () => navigate({ name: "approvals", focus: approval.id })
                      : undefined
                  }
                >
                  <span className="event-time">{clock(approval.requested_at)}</span>
                  <div className="row-main">
                    <div className="row-title mono">{approval.tool}</div>
                    <div className="row-meta">
                      <span>
                        <Visible text={approval.reason} />
                      </span>
                      {approval.decided_at ? (
                        <span>decided {clock(approval.decided_at)}</span>
                      ) : (
                        <span>waiting on you — open the card</span>
                      )}
                      {approval.note ? (
                        <span>
                          note: <Visible text={approval.note} />
                        </span>
                      ) : null}
                    </div>
                  </div>
                  <Risk level={approval.risk} />
                  <span className={`verdict ${approvalTone(approval.status)}`}>
                    <span className="visually-hidden">approval </span>
                    {status.charAt(0).toUpperCase() + status.slice(1)}
                  </span>
                </Row>
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}

/**
 * The audit records the run wrote, in chain order: the evidence behind the
 * trace. Each opens the record as the chain stores it.
 */
function AuditRecords({
  records,
  loading,
  error,
  live,
}: {
  records: readonly AuditRecordView[] | null;
  loading: boolean;
  error: string | null;
  live: boolean;
}) {
  const [openId, setOpenId] = useState<string | null>(null);
  return (
    <>
      <h2 id="run-audit">Audit records</h2>
      <p className="field-note">
        Every record this run wrote to the hash chain, in chain order. Each record's hash covers
        the one before it.
      </p>
      {error ? <ErrorBanner message={`The audit records could not be read: ${error}`} /> : null}
      <div className="panel">
        {loading ? <SkeletonRows count={2} /> : null}
        {records !== null && records.length === 0 ? (
          <Empty>
            {live ? "Nothing has been written to the chain yet." : "This run wrote no records."}
          </Empty>
        ) : null}
        {records !== null && records.length > 0 ? (
          <div role="list" aria-labelledby="run-audit">
            {records.map((record) => (
              <div role="listitem" key={record.id}>
                <AuditRow
                  record={record}
                  open={openId === record.id}
                  onToggle={() =>
                    setOpenId((current) => (current === record.id ? null : record.id))
                  }
                />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </>
  );
}

function AuditRow({
  record,
  open,
  onToggle,
}: {
  record: AuditRecordView;
  open: boolean;
  onToggle: () => void;
}) {
  const detailsId = `run-audit-${record.id}`;
  return (
    <>
      <button
        type="button"
        className={`row clickable event${record.security_relevant ? " security" : ""}`}
        aria-expanded={open}
        aria-controls={open ? detailsId : undefined}
        onClick={onToggle}
      >
        <span className="event-time">#{record.sequence}</span>
        <span className="event-time">{clock(record.at)}</span>
        {record.security_relevant ? (
          <span className="visually-hidden">security relevant: </span>
        ) : null}
        <span className="event-flag" aria-hidden="true">
          {record.security_relevant ? "!" : ""}
        </span>
        <span className="event-kind">{record.kind}</span>
        <span className="event-summary mono">
          <span className="visually-hidden">hash of the record before: </span>
          <span title={record.prev_hash}>{shortHash(record.prev_hash)}</span>
          <span aria-hidden="true"> → </span>
          <span className="visually-hidden">, this record's hash: </span>
          <span title={record.hash}>{shortHash(record.hash)}</span>
        </span>
      </button>
      {open ? <AuditRecord id={record.id} detailsId={detailsId} /> : null}
    </>
  );
}

/** One record in full, read from the chain when it is opened. */
function AuditRecord({ id, detailsId }: { id: string; detailsId: string }) {
  const record = useAsync(() => api.auditRecord(id), [id]);
  const data = record.data;
  return (
    <div id={detailsId} className="panel-body">
      {record.error ? <ErrorBanner message={record.error} /> : null}
      {record.loading ? <Loading what="the record" /> : null}
      {data ? (
        <>
          <dl className="facts">
            <dt>Sequence</dt>
            <dd>#{data.sequence}</dd>
            <dt>Record before it</dt>
            <dd className="mono">{data.prev_hash}</dd>
            <dt>This record</dt>
            <dd className="mono">{data.hash}</dd>
          </dl>
          <pre className="code wrap" tabIndex={0} role="region" aria-label="Record payload">
            <Visible text={data.payload} />
          </pre>
        </>
      ) : null}
    </div>
  );
}
