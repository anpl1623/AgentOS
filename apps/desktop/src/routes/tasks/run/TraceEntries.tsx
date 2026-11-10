import type { ReactNode } from "react";

import type { ExecutionView } from "../../../bindings/ExecutionView";
import type { StepView } from "../../../bindings/StepView";
import { ArgumentBlock, Visible } from "../../../components/ArgumentBlock";
import { CopyButton, Icon, LiveDot, Risk, Tainted, Verdict } from "../../../components/common";
import { clock, duration, humanise } from "../../../sdk/format";
import { marker } from "./runModel";
import type { SpanTarget } from "./timelineModel";
import {
  type NodeLook,
  callLook,
  firstLine,
  liveTone,
  sentence,
  stepLook,
  toolFamily,
} from "./traceLook";

/**
 * One stop on the trace's rail: the node it hangs on, and what happened.
 *
 * The rail runs through every node; from the first call made after the run
 * read untrusted data it is drawn dashed in the attention hue, matching the
 * mark on the timeline. The node is decoration — its icon and tone repeat
 * what the words beside it say.
 */
function Stop({
  id,
  look,
  tainted,
  failed,
  kind,
  children,
}: {
  id: string;
  look: NodeLook;
  tainted: boolean;
  failed: boolean;
  kind: "step" | "call";
  children: ReactNode;
}) {
  const classes = [
    "trace-item",
    `is-${kind}`,
    tainted ? "after-taint" : null,
    failed ? "failed-here" : null,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <li id={id} className={classes} tabIndex={-1}>
      <span className={`trace-node tone-${failed ? "danger" : look.tone}`} aria-hidden="true">
        <Icon name={failed ? "x" : look.icon} size={14} />
      </span>
      <div className="trace-body">{children}</div>
    </li>
  );
}

/** The note on the entry a run's failure is pinned to. */
function FailedHere({ failure }: { failure: string }) {
  return (
    <div className="banner error trace-failed" role="note">
      The run failed here: <Visible text={failure} />
    </div>
  );
}

/** One step: when, which, what kind, and what the runtime wrote about it. */
export function StepEntry({
  step,
  execution,
  tainted,
  failure,
  onReveal,
  children,
}: {
  step: StepView;
  /** The call the step reports, when it reports one the trace holds. */
  execution: ExecutionView | null;
  tainted: boolean;
  failure: string | null;
  onReveal: (target: SpanTarget) => void;
  /** Drawn beneath the summary: the delegations the step's call made. */
  children?: ReactNode;
}) {
  const look = stepLook(step.kind, execution);
  const { label } = marker(step.kind);
  const executionId = step.tool_execution_id;
  return (
    <Stop
      id={`run-step-${step.ordinal}`}
      look={look}
      tainted={tainted}
      failed={failure !== null}
      kind="step"
    >
      <div className="trace-head">
        <span className="trace-ordinal">{step.ordinal}</span>
        <span className="trace-kind" aria-hidden="true">
          {look.label}
        </span>
        <time className="trace-time" dateTime={step.at}>
          {clock(step.at)}
        </time>
        {executionId !== null ? (
          <button
            type="button"
            className="ghost trace-show-call"
            onClick={() => onReveal({ kind: "execution", id: executionId })}
          >
            Show call
            <Icon name="chevron-down" size={14} />
          </button>
        ) : null}
      </div>
      <div className="trace-summary prose">
        <span className="visually-hidden">{label}: </span>
        <Visible text={step.summary} />
      </div>
      {failure !== null ? <FailedHere failure={failure} /> : null}
      {children}
    </Stop>
  );
}

/**
 * One tool call, drawn as a card on the rail. Its head opens its details: the
 * exact arguments, the decision, the outcome, the duration, the call id and
 * the error in full.
 *
 * The arguments may fold long strings, since the call already happened and
 * nothing here can change what it did.
 */
export function CallEntry({
  execution,
  open,
  tainted,
  failure,
  onToggle,
  children,
}: {
  execution: ExecutionView;
  open: boolean;
  tainted: boolean;
  failure: string | null;
  onToggle: () => void;
  /** Drawn beneath the card whether or not it is open: the delegations the call made. */
  children?: ReactNode;
}) {
  const look = callLook(execution);
  const family = toolFamily(execution.tool);
  const detailsId = `run-execution-${execution.id}-details`;
  return (
    <Stop
      id={`run-execution-${execution.id}`}
      look={look}
      tainted={tainted}
      failed={failure !== null}
      kind="call"
    >
      <div className={`trace-call tone-${look.tone}${open ? " open" : ""}`}>
        <button
          type="button"
          className="trace-call-head"
          data-spotlight
          aria-expanded={open}
          aria-controls={open ? detailsId : undefined}
          onClick={onToggle}
        >
          <span className="trace-call-main">
            <span className="trace-call-title">
              <span className="visually-hidden">tool call: </span>
              <span className="trace-tool">{execution.tool}</span>
              <span className="trace-family" aria-hidden="true">
                {family.label}
              </span>
            </span>
            <span className="row-meta">
              <time dateTime={execution.started_at}>{clock(execution.started_at)}</time>
              <span>{duration(execution.duration_ms)}</span>
              <span>decision: {execution.effect}</span>
              {execution.tainted ? <Tainted label="after untrusted data" /> : null}
              {execution.error ? (
                <span className="muted">
                  <Visible text={firstLine(execution.error)} />
                </span>
              ) : null}
            </span>
          </span>
          <span className="trace-call-chips">
            <Risk level={execution.risk} />
            <Verdict outcome={execution.outcome} />
          </span>
          <Icon name="chevron-down" size={16} className="trace-chevron" />
        </button>
        {failure !== null ? <FailedHere failure={failure} /> : null}
        {open ? <CallDetails execution={execution} detailsId={detailsId} /> : null}
      </div>
      {children}
    </Stop>
  );
}

/** A call's details: its exact arguments, and the facts, each id a copy away. */
function CallDetails({ execution, detailsId }: { execution: ExecutionView; detailsId: string }) {
  return (
    <div id={detailsId} className="trace-call-details">
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
        <dd>
          <span className="mono">{execution.call_id}</span>
          <CopyButton text={execution.call_id} label="Copy the call id" />
        </dd>
        {execution.approval_id !== null ? (
          <>
            <dt>Approval</dt>
            <dd>
              <span className="mono">{execution.approval_id}</span>
              <CopyButton text={execution.approval_id} label="Copy the approval id" />
            </dd>
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
  );
}

/**
 * The delegations no recorded call accounts for. A call is recorded when it
 * ends, so on a live run they belong to the call still in progress, drawn
 * where that call will appear.
 */
export function UnplacedEntry({ live, children }: { live: boolean; children: ReactNode }) {
  const look: NodeLook = { icon: "agents", tone: live ? "info" : "neutral", label: "Delegation" };
  return (
    <Stop id="run-delegations-unplaced" look={look} tainted={false} failed={false} kind="call">
      <div className="trace-head">
        <span className="trace-tool">agent.delegate</span>
        <span className="muted">{live ? "in progress" : "with no recorded call"}</span>
      </div>
      {children}
    </Stop>
  );
}

/** The live end of the rail: where the run is now, pulsing until it ends. */
export function LiveEnd({ shown }: { shown: string }) {
  const tone = liveTone(shown) ?? "info";
  return (
    <li className="trace-item trace-live-end" aria-hidden="true">
      <span className={`trace-node tone-${tone}`}>
        <LiveDot tone={tone} />
      </span>
      <div className="trace-body">
        <div className="trace-head">
          <span className="trace-now">{sentence(shown)}…</span>
        </div>
      </div>
    </li>
  );
}
