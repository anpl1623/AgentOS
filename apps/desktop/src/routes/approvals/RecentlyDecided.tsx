import type { RefObject } from "react";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { Empty, ErrorBanner, Icon, Loading, Row, SkeletonRows } from "../../components/common";
import { ago, humanise } from "../../sdk/format";
import { decisionTone } from "./approvalsModel";

function sentence(value: string): string {
  const words = humanise(value);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * The requests answered most recently, by anyone, newest first. Each opens
 * its run, where what happened after the answer can be seen.
 */
export function RecentlyDecided({
  recent,
  headingRef,
  onOpen,
}: {
  recent: { data: readonly ApprovalView[] | null; error: string | null; loading: boolean };
  /** The heading, which a figure above the queue can send the keyboard to. */
  headingRef: RefObject<HTMLHeadingElement | null>;
  onOpen: (runId: string) => void;
}) {
  return (
    <section className="approvals-recent" aria-labelledby="approvals-recent-title">
      <h2 id="approvals-recent-title" tabIndex={-1} ref={headingRef}>
        Recently decided
      </h2>
      <div className="panel">
        {recent.error !== null && recent.data === null ? (
          <ErrorBanner message={recent.error} />
        ) : null}
        {recent.loading ? (
          <>
            <Loading what="recent decisions" />
            <SkeletonRows count={2} />
          </>
        ) : null}
        {recent.data?.length === 0 ? <Empty>Nothing has been decided yet.</Empty> : null}
        {recent.data !== null && recent.data.length > 0 ? (
          <div className="stagger" role="list" aria-label="Recently decided">
            {recent.data.map((approval) => {
              const tone = decisionTone(approval.status);
              return (
                <div role="listitem" key={approval.id}>
                  <Row className="recent-row" onActivate={() => onOpen(approval.run_id)}>
                    <span className={`recent-mark ${tone}`} aria-hidden="true">
                      <Icon
                        name={tone === "ok" ? "check" : tone === "blocked" ? "x" : "clock"}
                        size={14}
                      />
                    </span>
                    <div className="row-main">
                      <div className="row-title">
                        <span className="mono">{approval.tool}</span> for {approval.agent_name}
                      </div>
                      <div className="row-meta">
                        <span>
                          {approval.decided_at === null ? "undated" : ago(approval.decided_at)}
                        </span>
                        {approval.note === null ? null : <span>“{approval.note}”</span>}
                      </div>
                    </div>
                    <span className={`verdict ${tone}`}>
                      <span className="visually-hidden">decision </span>
                      {sentence(approval.status)}
                    </span>
                  </Row>
                </div>
              );
            })}
          </div>
        ) : null}
      </div>
      <p className="field-note">A decision cannot be withdrawn once the run has been told.</p>
    </section>
  );
}
