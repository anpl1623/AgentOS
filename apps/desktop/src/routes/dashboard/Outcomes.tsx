import type { TaskSummary } from "../../bindings/TaskSummary";
import {
  Empty,
  Icon,
  Risk,
  Row,
  SkeletonRows,
  Status,
  Tainted,
  Verdict,
} from "../../components/common";
import { ago } from "../../sdk/format";
import type { Navigate } from "../route";
import { type RefusalRow, failureRows } from "./dashboardModel";
import { Item, List, Section, SeeAll } from "./Section";

/** Placeholder rows on first paint. */
const SKELETON_ROWS = 3;

/**
 * Tasks whose latest attempt failed, most recent first. Each row leads with a
 * mark in the danger hue and opens the failed run's trace; a task that never
 * ran has no trace, so its row is not a button and says why instead.
 */
export function Failed({
  tasks,
  first,
  navigate,
}: {
  tasks: readonly TaskSummary[] | null;
  first: boolean;
  navigate: Navigate;
}) {
  return (
    <Section
      id="dashboard-failed"
      title="Failed recently"
      icon="x"
      className="dash-failed"
      count={tasks && tasks.length > 0 ? tasks.length : null}
    >
      <div className="panel">
        {first ? (
          <SkeletonRows count={1} />
        ) : !tasks ? null : tasks.length === 0 ? (
          <Empty>Nothing has failed recently.</Empty>
        ) : (
          <List labelledBy="dashboard-failed" className="stagger">
            {failureRows(tasks).map((failure) => {
              const runId = failure.runId;
              return (
                <Item key={failure.id}>
                  <Row
                    className="dash-outcome failed"
                    onActivate={runId ? () => navigate({ name: "tasks", runId }) : undefined}
                  >
                    <span className="dash-outcome-mark" aria-hidden="true">
                      <Icon name="x" size={14} />
                    </span>
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
      </div>
    </Section>
  );
}

/**
 * Calls the policy, a person or the approval budget refused, newest first.
 *
 * Drawn in the blocked hue rather than danger: a refusal is the policy doing
 * its job. Each row opens the run that made the call.
 */
export function Refused({
  refusals,
  loaded,
  first,
  navigate,
}: {
  refusals: readonly RefusalRow[];
  /** The dashboard has answered at least once. */
  loaded: boolean;
  first: boolean;
  navigate: Navigate;
}) {
  return (
    <Section
      id="dashboard-refused"
      title="Recently refused"
      icon="lock"
      className="dash-refused"
      count={refusals.length > 0 ? refusals.length : null}
      tail={<SeeAll label="Activity" onActivate={() => navigate({ name: "activity" })} />}
    >
      <div className="panel">
        {first ? (
          <SkeletonRows count={SKELETON_ROWS} />
        ) : !loaded ? null : refusals.length === 0 ? (
          <Empty>Nothing has been refused.</Empty>
        ) : (
          <List labelledBy="dashboard-refused" className="stagger">
            {refusals.map((refusal) => (
              <Item key={refusal.id}>
                <Row
                  className="dash-outcome refused"
                  onActivate={() => navigate({ name: "tasks", runId: refusal.runId })}
                >
                  <span className="dash-outcome-mark" aria-hidden="true">
                    <Icon name="lock" size={14} />
                  </span>
                  <div className="row-main">
                    <div className="row-title mono">{refusal.tool}</div>
                    <div className="row-meta">
                      <span title={refusal.detail}>{refusal.reason}</span>
                      <span>{ago(refusal.startedAt)}</span>
                      {refusal.tainted ? <Tainted /> : null}
                    </div>
                  </div>
                  <span className="dash-outcome-chips">
                    <Risk level={refusal.risk} />
                    <Verdict outcome={refusal.outcome} />
                  </span>
                </Row>
              </Item>
            ))}
          </List>
        )}
      </div>
    </Section>
  );
}
