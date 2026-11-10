import type { ApprovalView } from "../../../bindings/ApprovalView";
import { Visible } from "../../../components/ArgumentBlock";
import { Icon, type IconName, LiveDot, Risk, Row } from "../../../components/common";
import { clock } from "../../../sdk/format";
import type { Navigate } from "../../route";
import { approvalTone } from "./runModel";
import { sentence } from "./traceLook";

/** A decided approval's mark: a check for approved, a cross for denied. */
function decidedIcon(status: string): IconName {
  return status === "approved" ? "check" : status === "denied" ? "x" : "info";
}

/** Approvals the run asked for. A pending one opens its card. */
export function RunApprovals({
  approvals,
  navigate,
}: {
  approvals: readonly ApprovalView[];
  navigate: Navigate;
}) {
  return (
    <section className="run-approvals" aria-labelledby="run-approvals">
      <div className="run-section-head">
        <h2 id="run-approvals">Approvals</h2>
        <span className="run-count" aria-hidden="true">
          {approvals.length}
        </span>
      </div>
      <div className="panel">
        <div role="list" aria-labelledby="run-approvals">
          {approvals.map((approval) => {
            const tone = approvalTone(approval.status);
            const pending = approval.status === "pending";
            return (
              <div
                role="listitem"
                key={approval.id}
                id={`run-approval-${approval.id}`}
                tabIndex={-1}
              >
                <Row
                  className={pending ? "run-approval pending" : "run-approval"}
                  onActivate={
                    pending ? () => navigate({ name: "approvals", focus: approval.id }) : undefined
                  }
                >
                  {/* A pending request's mark is in the attention hue, as its
                      card and the approvals count are; the chip beside it
                      keeps the state's own tone. */}
                  <span
                    className={`run-row-mark tone-${pending ? "warn" : tone}`}
                    aria-hidden="true"
                  >
                    {pending ? (
                      <LiveDot tone="warn" />
                    ) : (
                      <Icon name={decidedIcon(approval.status)} size={14} />
                    )}
                  </span>
                  <div className="row-main">
                    <div className="row-title mono">{approval.tool}</div>
                    <div className="row-meta">
                      <time dateTime={approval.requested_at}>{clock(approval.requested_at)}</time>
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
                  <span className={`verdict ${tone}`}>
                    <span className="visually-hidden">approval </span>
                    {sentence(approval.status)}
                  </span>
                  {pending ? (
                    <Icon name="chevron-right" size={16} className="run-row-go" />
                  ) : null}
                </Row>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
