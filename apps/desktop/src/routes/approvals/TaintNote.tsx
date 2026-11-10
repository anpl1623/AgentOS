import type { ApprovalView } from "../../bindings/ApprovalView";
import { Visible } from "../../components/ArgumentBlock";
import { Icon } from "../../components/common";
import { askedOnlyForTaint } from "./approvalsModel";
import { taintOrigin } from "./deskModel";

/**
 * Why a run that has read untrusted data is being asked, and what it read.
 *
 * A note the article names in `aria-describedby`, so the warning is read
 * before the decision buttons are reached rather than after, and above the
 * facts it casts doubt on. Each source leads with its kind (a web page, a
 * file, a terminal) so the eye finds where the data came from before reading
 * the address; the address itself is drawn exactly, hidden characters boxed.
 */
export function TaintNote({ id, approval }: { id: string; approval: ApprovalView }) {
  const sources = approval.taint_sources;
  const onlyTaint = askedOnlyForTaint(approval);
  return (
    <div className="approval-warning" role="note" id={id}>
      <p className="approval-warning-title">
        <Icon name="alert" size={15} />
        Read untrusted data
      </p>
      <p className="approval-warning-text">
        {onlyTaint
          ? sources.length > 0
            ? "This would have run without asking. It is being asked because this run read:"
            : "This would have run without asking. It is being asked because this run has read data from outside the trust boundary."
          : "This agent has read data from outside the trust boundary during this run. Whatever it is proposing may have been influenced by that content rather than by your objective."}
      </p>
      {sources.length > 0 ? (
        <ul className="taint-origins" role="list" aria-label="Untrusted sources it read">
          {sources.map((source) => {
            const origin = taintOrigin(source);
            return (
              <li key={source} className="taint-origin">
                <Icon name={origin.icon} size={14} />
                {origin.kind === null ? null : (
                  <span className="taint-origin-kind">
                    <Visible text={origin.kind} />
                    <span className="visually-hidden">:</span>
                  </span>
                )}
                <span className="taint-origin-detail">
                  <Visible text={origin.detail} />
                </span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
