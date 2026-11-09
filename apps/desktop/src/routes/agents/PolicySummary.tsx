import type { RefObject } from "react";

import type { PolicyView } from "../../bindings/PolicyView";
import { Empty, Icon, Risk } from "../../components/common";
import { Item, List } from "./lists";
import { effectParts, ruleTone, splitRule } from "./policyModel";

/**
 * A stored policy, read: its version, default, ceiling and taint escalation
 * across the top, then each rule in the engine's own words, laid out as what
 * it names and what it does. The effect's verb is a chip in its tone, so an
 * `allow` and a `deny` are told apart at a glance and by their words.
 */
export function PolicySummary({
  policy,
  editRef,
  onEdit,
}: {
  policy: PolicyView;
  /** The Edit button, so focus can come back to it when the editor closes. */
  editRef: RefObject<HTMLButtonElement | null>;
  onEdit: () => void;
}) {
  return (
    <div className="panel policy-card">
      <div className="policy-head">
        <dl className="policy-facts">
          <div>
            <dt>Version</dt>
            <dd className="mono">{policy.version}</dd>
          </div>
          <div>
            <dt>Default</dt>
            <dd>
              <b>{policy.default_effect}</b>
            </dd>
          </div>
          {policy.max_risk ? (
            <div>
              <dt>Ceiling</dt>
              <dd>
                <Risk level={policy.max_risk} />
              </dd>
            </div>
          ) : null}
          <div>
            <dt>Taint</dt>
            <dd>
              {policy.taint_enabled ? (
                <>
                  after reading untrusted data, <b>{policy.taint_threshold}</b>+ needs approval
                </>
              ) : (
                <span className="verdict warn">Taint escalation off</span>
              )}
            </dd>
          </div>
        </dl>
        <button ref={editRef} type="button" onClick={onEdit}>
          <Icon name="edit" size={15} />
          Edit
        </button>
      </div>
      {policy.rules.length === 0 ? (
        <Empty>No rules. The default decides every call.</Empty>
      ) : (
        <List label="Rules" className="rule-list">
          {policy.rules.map((rule, index) => {
            const line = splitRule(rule);
            const effect = effectParts(line.effect);
            return (
              <Item key={`${index}:${rule}`}>
                <div className="rule">
                  <span className="rule-capability mono">{line.capability}</span>
                  {effect.verb !== "" ? (
                    <span className="rule-effect">
                      <span className={`verdict ${ruleTone(effect.verb)}`}>{effect.verb}</span>
                      {effect.rest !== "" ? <span className="mono">{effect.rest}</span> : null}
                    </span>
                  ) : null}
                </div>
              </Item>
            );
          })}
        </List>
      )}
    </div>
  );
}
