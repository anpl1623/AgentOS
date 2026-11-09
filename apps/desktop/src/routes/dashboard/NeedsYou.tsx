import type { ApprovalView } from "../../bindings/ApprovalView";
import {
  EmptyState,
  ErrorBanner,
  Icon,
  Risk,
  SkeletonRows,
  Tainted,
  riskTone,
} from "../../components/common";
import type { PendingApprovals } from "../../sdk/approvals";
import type { Navigate } from "../route";
import { needsYou, span } from "./dashboardModel";
import { decisionSummary, waitedLong } from "./overviewModel";
import { Item, List, Section, SeeAll } from "./Section";

/**
 * What is waiting on the person, as decision cards: the loudest thing on the
 * screen, in the attention hue the approvals count wears in the sidebar.
 *
 * Each card says who wants to do what, how dangerous it is (an edge in the
 * risk's hue, glowing for high and critical), whether the run has read
 * untrusted data, and how long the run has been blocked on it. The card opens
 * the approval on the approvals screen; nothing here can approve or deny,
 * because a decision costs a deliberate click where the whole request can be
 * read.
 */
export function NeedsYou({
  pending,
  now,
  navigate,
}: {
  pending: PendingApprovals;
  now: number;
  navigate: Navigate;
}) {
  const queue = needsYou(pending.approvals);
  return (
    <Section
      id="dashboard-needs-you"
      title="Needs you"
      icon="approvals"
      className="dash-needs"
      count={pending.loaded ? pending.approvals.length : null}
      tail={
        pending.loaded && pending.approvals.length > 0 ? (
          <SeeAll label="Approvals" onActivate={() => navigate({ name: "approvals" })} />
        ) : null
      }
      above={
        pending.error ? (
          <ErrorBanner message={`Waiting approvals could not be read: ${pending.error}`} />
        ) : null
      }
    >
      {!pending.loaded ? (
        pending.error ? null : (
          <div className="panel">
            <SkeletonRows count={1} />
          </div>
        )
      ) : queue.shown.length === 0 ? (
        <div className="panel">
          <EmptyState icon="check" title="Nothing is waiting for you.">
            When an agent asks before acting, its request appears here first.
          </EmptyState>
        </div>
      ) : (
        <List labelledBy="dashboard-needs-you" className="dash-decisions stagger">
          {queue.shown.map((approval) => (
            <Item key={approval.id}>
              <DecisionCard approval={approval} now={now} navigate={navigate} />
            </Item>
          ))}
          {queue.more > 0 ? (
            <Item>
              <button
                type="button"
                className="dash-decision-more lift"
                data-spotlight
                onClick={() => navigate({ name: "approvals" })}
              >
                <span className="dash-decision-more-count">+{queue.more}</span>
                more waiting
                <span className="visually-hidden"> — open Approvals</span>
                <Icon name="chevron-right" size={14} />
              </button>
            </Item>
          ) : null}
        </List>
      )}
    </Section>
  );
}

/**
 * One request waiting on a decision. A real button, so the whole card is one
 * target and one tab stop; everything inside is phrasing content, as a
 * button's content must be.
 */
function DecisionCard({
  approval,
  now,
  navigate,
}: {
  approval: ApprovalView;
  now: number;
  navigate: Navigate;
}) {
  const long = waitedLong(approval.requested_at, now);
  return (
    <button
      type="button"
      className={`dash-decision risk-${riskTone(approval.risk)} lift`}
      data-spotlight
      onClick={() => navigate({ name: "approvals", focus: approval.id })}
    >
      <span className="dash-decision-head">
        <span className="dash-decision-who mono">
          {approval.agent_name}
          <span className="dash-decision-sep" aria-hidden="true">
            ·
          </span>
          <span className="visually-hidden"> wants </span>
          {approval.tool}
        </span>
        <Risk level={approval.risk} />
      </span>
      <span className="dash-decision-what">{decisionSummary(approval)}</span>
      <span className="dash-decision-foot">
        <span className={long ? "dash-decision-wait long" : "dash-decision-wait"}>
          <Icon name="clock" size={14} />
          blocked for {span(approval.requested_at, now)}
        </span>
        {approval.tainted ? <Tainted /> : null}
        <span className="dash-decision-go">
          Review
          <Icon name="chevron-right" size={14} />
        </span>
      </span>
    </button>
  );
}
