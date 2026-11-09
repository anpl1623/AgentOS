import type { PolicyView } from "../../bindings/PolicyView";
import { Empty, ErrorBanner, Icon, Risk, Row } from "../../components/common";
import { domainIcon } from "./agentsModel";
import {
  type GrantRow,
  REACH_LEGEND,
  deadGrantText,
  reachLabel,
  reachTone,
} from "./grantsModel";
import { Item, List } from "./lists";
import { Section } from "./Section";

/**
 * The tools an agent holds and how far its policy lets each capability reach.
 *
 * A grant every call of which will be refused is the loudest thing in the
 * section: a danger edge on its row, the reason in words beneath it, and a
 * chip naming it. The key above the rows says what each reach means, drawn
 * with the same chips the rows use.
 */
export function GrantsSection({
  rows,
  policy,
  reportError,
  catalogueError,
}: {
  rows: readonly GrantRow[];
  policy: PolicyView | null;
  reportError: string | null;
  catalogueError: string | null;
}) {
  return (
    <Section
      id="agent-tools"
      title="Tools & grants"
      icon="zap"
      aside={<span className="section-count">{rows.length}</span>}
    >
      <p className="section-lede">
        Granting a tool offers it to the model; the policy decides each call. Beside each
        capability is how far the permission engine reads the saved policy as letting it reach.
        That reading can overstate and never understates.
      </p>
      <dl className="reach-legend" aria-label="What each reach means">
        {REACH_LEGEND.map(({ reach, meaning }) => (
          <div key={reach} className="reach-key">
            <dt>
              <span className={`verdict ${reachTone(reach)}`}>{reachLabel(reach)}</span>
            </dt>
            <dd>{meaning}</dd>
          </div>
        ))}
      </dl>

      {reportError ? (
        <ErrorBanner message={`How far this policy reaches could not be read: ${reportError}`} />
      ) : null}
      {catalogueError ? (
        <ErrorBanner message={`The tool catalogue could not be read: ${catalogueError}`} />
      ) : null}
      <div className="panel">
        {rows.length === 0 ? (
          <Empty>No tools granted.</Empty>
        ) : (
          <List labelledBy="agent-tools">
            {rows.map((row) => (
              <Item key={row.tool}>
                <GrantLine row={row} policy={policy} />
              </Item>
            ))}
          </List>
        )}
      </div>
    </Section>
  );
}

/**
 * One granted tool: what it is, what it may cost, and how far the policy lets
 * each of its capabilities reach. A grant every call of which will be refused
 * says so on the row, in danger, with the reason.
 */
function GrantLine({ row, policy }: { row: GrantRow; policy: PolicyView | null }) {
  const dead = deadGrantText(row, policy);
  return (
    <Row className={dead ? "grant dead" : "grant"}>
      <span className="grant-icon" aria-hidden="true">
        <Icon name={domainIcon(row.domain)} size={16} />
      </span>
      <div className="row-main">
        <div className="row-title mono">{row.tool}</div>
        {row.description ? (
          <div className="row-meta">
            <span>{row.description}</span>
          </div>
        ) : null}
        {row.capabilities.length > 0 ? (
          <div className="grant-caps">
            {row.capabilities.map(({ capability, reach }) => (
              <span key={capability} className="grant-cap">
                <span className="mono">{capability}</span>
                {reach !== null ? (
                  <span className={`verdict ${reachTone(reach)}`}>
                    <span className="visually-hidden">reach </span>
                    {reachLabel(reach)}
                  </span>
                ) : null}
              </span>
            ))}
          </div>
        ) : null}
        {dead ? (
          <div className="grant-refusal">
            <Icon name="lock" size={14} />
            <span>{dead.sentence}</span>
          </div>
        ) : null}
      </div>
      <div className="grant-chips">
        {row.risk !== null ? <Risk level={row.risk} /> : null}
        {row.external ? (
          <span className="verdict warn">
            <span className="visually-hidden">reads </span>external
          </span>
        ) : null}
        {dead ? (
          <span className="verdict danger">
            <span className="visually-hidden">grant </span>
            {dead.label}
          </span>
        ) : null}
      </div>
    </Row>
  );
}
