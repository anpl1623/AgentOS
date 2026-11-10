import type { ApprovalView } from "../../../bindings/ApprovalView";
import type { TraceView } from "../../../bindings/TraceView";
import { Visible } from "../../../components/ArgumentBlock";
import { ErrorBanner, Icon, LiveDot, Risk } from "../../../components/common";
import type { Navigate } from "../../route";
import type { FailureAnchor } from "./runModel";

/**
 * What the run wants read before its trace, loudest first: that a stop is
 * not yet a stop, that a read or an action failed, that the run is waiting on
 * the person reading, that it read untrusted data, and how it ended.
 */
export function RunNotices({
  trace,
  shown,
  refreshError,
  actionError,
  pending,
  anchor,
  onReveal,
  navigate,
}: {
  trace: TraceView;
  shown: string;
  /** The last refresh's failure, while an earlier trace stays on screen. */
  refreshError: string | null;
  actionError: string | null;
  pending: readonly ApprovalView[];
  anchor: FailureAnchor | null;
  onReveal: (anchor: FailureAnchor) => void;
  navigate: Navigate;
}) {
  const { run } = trace;
  return (
    <>
      {/* The live region is always present and only its content comes and
          goes: a region inserted with its message already in it is not
          reliably read out, and this is the answer to pressing Stop. */}
      <div role="status">
        {shown === "cancelling" ? (
          <p className="banner warn run-cancelling">
            <LiveDot tone="warn" />
            <span>
              Asked to stop. The run has not ended yet: a tool call already in progress finishes
              or is abandoned before it does.
            </span>
          </p>
        ) : null}
      </div>
      {refreshError ? (
        <ErrorBanner message={`The trace could not be refreshed: ${refreshError}`} />
      ) : null}
      {actionError ? <ErrorBanner message={actionError} /> : null}

      {pending.length > 0 ? (
        <WaitingOnYou approvals={pending} navigate={navigate} />
      ) : null}

      {run.tainted && run.taint_sources.length > 0 ? (
        <p className="banner warn run-taint">
          <span className="run-banner-icon" aria-hidden="true">
            <Icon name="alert" size={16} />
          </span>
          <span>
            Untrusted data came from{" "}
            {run.taint_sources.map((source, index) => (
              <span key={source}>
                {index > 0 ? ", " : null}
                <span className="mono">
                  <Visible text={source} />
                </span>
              </span>
            ))}
            .
          </span>
        </p>
      ) : null}

      {run.failure ? (
        <div className="banner error run-failure">
          <span className="run-banner-icon" aria-hidden="true">
            <Icon name="x" size={16} />
          </span>
          <span className="prose run-failure-text">
            <Visible text={run.failure} />
          </span>
          {anchor !== null ? (
            <button type="button" className="ghost" onClick={() => onReveal(anchor)}>
              Show where it failed →
            </button>
          ) : null}
        </div>
      ) : null}

      {run.result ? (
        <section className="run-result" aria-labelledby="run-result">
          <h2 id="run-result">
            <Icon name="check" size={14} />
            Result
          </h2>
          <div className="panel">
            <div className="panel-body prose">
              <Visible text={run.result} />
            </div>
          </div>
        </section>
      ) : null}
    </>
  );
}

/**
 * The decision this run is waiting for, above everything it did.
 *
 * The one card on the screen that glows: a run waiting on a person stands
 * still until they answer, and the answer is given on the approval card, not
 * here, so this only takes them there.
 */
function WaitingOnYou({
  approvals,
  navigate,
}: {
  approvals: readonly ApprovalView[];
  navigate: Navigate;
}) {
  const first = approvals[0];
  if (first === undefined) return null;
  const more = approvals.length - 1;
  return (
    <section className="run-waiting" aria-labelledby="run-waiting-title">
      <span className="run-waiting-mark" aria-hidden="true">
        <Icon name="approvals" size={18} />
      </span>
      <div className="run-waiting-body">
        <p id="run-waiting-title" className="run-waiting-title">
          Waiting on you to decide <span className="mono">{first.tool}</span>
          {more > 0 ? ` and ${more} more` : null}
        </p>
        <p className="run-waiting-reason">
          <Visible text={first.explanation || first.reason} />
        </p>
      </div>
      <Risk level={first.risk} />
      <button
        type="button"
        className="primary"
        onClick={() => navigate({ name: "approvals", focus: first.id })}
      >
        Review the request
        <Icon name="chevron-right" size={16} />
      </button>
    </section>
  );
}
