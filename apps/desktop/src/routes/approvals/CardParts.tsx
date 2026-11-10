import type { CSSProperties } from "react";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { ArgumentBlock, Visible } from "../../components/ArgumentBlock";
import { ErrorBanner, Icon, Risk } from "../../components/common";
import { blockedFor, budgetLine, chainLine, describedWarning } from "./approvalsModel";
import { budgetMeter, matchedRule, ruleEffect, toolIcon } from "./deskModel";
import { TaintNote } from "./TaintNote";

/**
 * The top of an approval card: who wants to do what, how long their run has
 * been held, how much of its approval budget it has used, and how dangerous
 * the action is. Shared by the waiting card and the copy of it that is
 * stamped and folded away once answered, so the two read identically.
 */
export function CardHead({
  approval,
  now,
  titleId,
  onDrill,
}: {
  approval: ApprovalView;
  now: number;
  titleId: string;
  /** Opens the run; left out on a card that has been answered. */
  onDrill?: (() => void) | undefined;
}) {
  const chain = chainLine(approval.delegation_chain);
  const budget = budgetLine(approval.asked_this_run, approval.approval_budget);
  const meter = budgetMeter(approval.asked_this_run, approval.approval_budget);
  return (
    <div className="approval-head">
      <span className="approval-mark" aria-hidden="true">
        <Icon name={toolIcon(approval.tool)} size={18} />
      </span>
      <div className="approval-who">
        <h2 id={titleId} className="approval-title">
          {approval.agent_name} wants to run{" "}
          <span className="mono approval-tool">{approval.tool}</span>
        </h2>
        <div className="row-meta approval-meta">
          {chain === null ? null : (
            <span className="approval-fact">
              <Icon name="user" size={13} />
              <span>
                <span className="visually-hidden">delegated: </span>
                {chain}
              </span>
            </span>
          )}
          <span className="approval-fact">
            <Icon name="clock" size={13} />
            <span>blocked for {blockedFor(approval.requested_at, now)}</span>
          </span>
          {budget === null ? null : (
            <span className="approval-fact">
              {meter === null ? null : (
                <span
                  className={`budget-meter ${meter.tone}`}
                  style={{ "--fill": `${Math.round(meter.fill * 100)}%` } as CSSProperties}
                  aria-hidden="true"
                />
              )}
              <span>{budget}</span>
            </span>
          )}
        </div>
      </div>
      <div className="approval-aside">
        <Risk level={approval.risk} />
        {onDrill === undefined ? null : (
          <button type="button" className="ghost approval-drill" onClick={onDrill}>
            What it has done so far
            <Icon name="chevron-right" size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * Everything a decision needs, inside the card's scrolling body: the plain
 * summary, the taint warning, the objective, the rule that matched and why,
 * what it touches, and the exact arguments.
 *
 * `idPrefix` keeps element ids unique when the same request is drawn twice,
 * as it is for a moment while its answered copy folds away.
 */
export function CardBody({
  approval,
  idPrefix,
  stop,
  failure,
}: {
  approval: ApprovalView;
  idPrefix: string;
  /** Whether the card offers to stop the run, which the body explains. */
  stop: boolean;
  /** Why the last answer to this card failed, if it did. */
  failure: string | null;
}) {
  const described = describedWarning(approval);
  const rule = matchedRule(approval.reason);
  const effect = ruleEffect(approval);
  return (
    <>
      {described !== null ? (
        <div className="banner warn" role="note">
          {described}
        </div>
      ) : null}
      <p className="approval-explanation">
        <Visible text={approval.explanation} />
      </p>

      {approval.tainted ? <TaintNote id={`${idPrefix}-taint`} approval={approval} /> : null}

      <dl className="facts approval-facts">
        <dt>Working on</dt>
        <dd>
          <Visible text={approval.objective} />
        </dd>

        {rule === null && effect === null ? null : (
          <>
            <dt>Rule</dt>
            <dd className="approval-rule">
              {rule === null ? null : (
                <span className="rule-chip">
                  <Icon name="shield" size={13} />
                  <Visible text={rule} />
                </span>
              )}
              {effect === null ? null : <span className="muted">{effect}</span>}
            </dd>
          </>
        )}

        <dt>Because</dt>
        <dd>
          <Visible text={approval.reason} />
        </dd>

        {approval.affected_resources.length > 0 ? (
          <>
            <dt>Affects</dt>
            <dd>
              <div className="inline" role="list" aria-label="Resources it affects">
                {approval.affected_resources.map((resource) => (
                  <span key={resource} className="tag" role="listitem">
                    <Visible text={resource} />
                  </span>
                ))}
              </div>
            </dd>
          </>
        ) : null}
      </dl>

      <ArgumentBlock json={approval.arguments} id={`${idPrefix}-args`} />

      {failure !== null ? <ErrorBanner message={failure} /> : null}

      {stop ? (
        <p className="faint approval-stop-note">
          Refusing one call leaves the run reasoning from the same context that produced it. Deny
          and stop the run ends it as well.
        </p>
      ) : null}
    </>
  );
}
