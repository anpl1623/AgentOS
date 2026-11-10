import type { RunSummary } from "../../../bindings/RunSummary";
import type { TraceView } from "../../../bindings/TraceView";
import { elapsed } from "../../../components/charts";
import {
  CopyButton,
  Icon,
  LiveDot,
  PageHeader,
  Stale,
  State,
  StatTile,
  Tainted,
} from "../../../components/common";
import { useCopy } from "../../../components/hooks";
import { ago, clock, tokens } from "../../../sdk/format";
import type { Navigate } from "../../route";
import { attemptsLabel } from "./runModel";
import { liveTone, runElapsed, stateIcon, stateTone } from "./traceLook";
import { transcript } from "./transcriptModel";
import type { RunActions } from "./useRunControls";

/**
 * The top of a run: where it sits, what it was asked, and the two things an
 * operator may need to do at once — copy it out as evidence, or stop it.
 *
 * The actions sit in the page header's title row, which pins while the trace
 * scrolls, so Stop is never more than a glance away on a long run.
 */
export function RunHeader({
  trace,
  shown,
  live,
  retryable,
  actions,
  navigate,
}: {
  trace: TraceView;
  /** The state to show: the run's own, or `cancelling` once a stop is asked for. */
  shown: string;
  live: boolean;
  retryable: boolean;
  actions: RunActions;
  navigate: Navigate;
}) {
  const { run } = trace;
  const tone = liveTone(shown);
  return (
    <PageHeader
      clampPinned
      parent={{ label: "Tasks", onActivate: () => navigate({ name: "tasks" }) }}
      eyebrow={
        <span className="run-eyebrow">
          {tone !== null ? <LiveDot tone={tone} /> : null}
          <span>{live ? "Live run" : "Run"}</span>
          <span className="run-eyebrow-id mono">{run.id}</span>
          <CopyButton text={run.id} label="Copy the run id" />
        </span>
      }
      title={trace.objective}
      subtitle={
        <>
          {trace.agent_name} · attempt {run.attempt} · started {ago(run.started_at)}
          {run.completed_at ? ` · ended ${ago(run.completed_at)}` : null}
        </>
      }
      actions={
        <>
          <CopyRunButton trace={trace} shown={shown} />
          {live ? (
            <StopButton requested={actions.cancelRequested} onStop={actions.cancel} />
          ) : retryable ? (
            // Disabled outright only when there is no task to retry; while a
            // retry is under way it stays focusable (see StopButton).
            <button
              type="button"
              className="primary"
              disabled={trace.task_id === null}
              aria-disabled={actions.retrying || undefined}
              onClick={actions.retrying ? undefined : () => void actions.retry()}
            >
              <Icon name="retry" size={16} />
              {actions.retrying ? "Retrying…" : "Retry"}
            </button>
          ) : null}
        </>
      }
    />
  );
}

/**
 * Stop: a safety control, so it acts on the first press and is drawn to be
 * found without looking for it — a filled stop mark, the danger hue at rest
 * rather than only on hover, and the words saying exactly what it stops.
 * Pressed, it holds its place and says it is stopping; the state chip says
 * `cancelling` until the runtime reports the run ended.
 *
 * While the stop is asked for it is aria-disabled, not disabled: a focused
 * button that becomes disabled drops focus to the top of the window, and the
 * person who pressed Stop would lose their place on the page at the moment
 * they most need it. The press is ignored here and in the action itself.
 */
function StopButton({ requested, onStop }: { requested: boolean; onStop: () => Promise<void> }) {
  return (
    <button
      type="button"
      className="danger run-stop"
      aria-disabled={requested || undefined}
      onClick={requested ? undefined : () => void onStop()}
    >
      <span className="run-stop-mark" aria-hidden="true">
        <Icon name="stop" size={14} />
      </span>
      {requested ? "Stopping…" : "Stop this agent"}
    </button>
  );
}

/** The run as text, for a ticket; the button says what happened and a hidden line says it aloud. */
function CopyRunButton({ trace, shown }: { trace: TraceView; shown: string }) {
  const { copy, state } = useCopy();
  return (
    <>
      <button
        type="button"
        className="run-copy"
        data-state={state}
        onClick={() => void copy(transcript(trace, shown))}
      >
        <Icon
          name={state === "copied" ? "check" : state === "failed" ? "alert" : "copy"}
          size={16}
        />
        {state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : "Copy this run"}
      </button>
      <span className="visually-hidden" role="status">
        {state === "copied"
          ? "The run was copied as text."
          : state === "failed"
            ? "The run could not be copied."
            : ""}
      </span>
    </>
  );
}

/**
 * The run at a glance: its state, how long it has taken, how far it has got,
 * what it has cost, and which attempt this is.
 *
 * Only the state tile takes a tone, and only a loud one when the run needs a
 * person. The figures count to their values and, on a live run, onwards as
 * the run moves.
 */
export function RunStats({
  trace,
  shown,
  now,
  calls,
  attempts,
  stale,
}: {
  trace: TraceView;
  shown: string;
  now: number;
  /** How many tool calls the trace holds. */
  calls: number;
  attempts: readonly RunSummary[];
  /** The stale note, when the last refresh failed. */
  stale: number | null;
}) {
  const { run } = trace;
  const took = runElapsed(run, now);
  const total = run.input_tokens + run.output_tokens;
  return (
    <section className="run-stats" aria-label="This run at a glance">
      <div className="run-stats-grid">
        <div className={`stat-tile tone-${stateTone(shown)} run-state-tile`}>
          <span className="stat-tile-head">
            <span className="stat-tile-icon" aria-hidden="true">
              <Icon name={stateIcon(shown)} size={16} />
            </span>
            <span className="stat-tile-label">State</span>
          </span>
          <span className="run-state-value">
            {shown === "cancelling" ? (
              <span className="verdict cancelling">
                <span className="visually-hidden">run </span>Cancelling
              </span>
            ) : (
              <State state={run.state} />
            )}
            {run.tainted ? <Tainted /> : null}
          </span>
          {stale !== null ? (
            <span className="stat-tile-foot">
              <Stale since={stale} />
            </span>
          ) : null}
        </div>
        <StatTile
          label="Elapsed"
          icon="clock"
          value={took}
          format={elapsed}
          hint={
            run.completed_at ? `ended ${clock(run.completed_at)}` : `from ${clock(run.started_at)}`
          }
        />
        <StatTile
          label="Steps"
          icon="tasks"
          value={run.steps}
          hint={calls === 1 ? "1 tool call" : `${calls} tool calls`}
        />
        <StatTile
          label="Tokens"
          icon="cpu"
          value={total}
          format={tokens}
          hint={`${tokens(run.input_tokens)} in · ${tokens(run.output_tokens)} out`}
        />
        <StatTile
          label="Attempt"
          icon="retry"
          value={run.attempt}
          hint={`of ${Math.max(attempts.length, run.attempt)}`}
        />
      </div>
    </section>
  );
}

/** The attempt switcher: every attempt at the task, each opening its own run. */
export function Attempts({
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
    <div className="run-attempts" role="group" aria-label="Attempts">
      <span className="run-attempts-label">{label}</span>
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
