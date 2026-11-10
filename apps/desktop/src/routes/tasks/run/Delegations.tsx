import type { CSSProperties } from "react";

import type { DelegationView } from "../../../bindings/DelegationView";
import { Visible } from "../../../components/ArgumentBlock";
import { Icon, Tainted } from "../../../components/common";
import type { Navigate } from "../../route";
import type { DelegationRow } from "./delegationModel";
import { sentence } from "./traceLook";

/**
 * The run that asked for this one, above everything it did.
 *
 * A delegated run read on its own looks like a task nobody set; this line is
 * the way back up to the run that set it.
 */
export function DelegatedBy({
  delegation,
  navigate,
}: {
  delegation: DelegationView;
  navigate: Navigate;
}) {
  return (
    <p className="run-delegated-by">
      <Icon name="agents" size={14} />
      <span>
        Started by <strong>{delegation.parent_agent}</strong>
      </span>
      <button
        type="button"
        className="ghost mono"
        onClick={() => navigate({ name: "tasks", runId: delegation.parent_run_id })}
      >
        <span className="visually-hidden">open the trace of the run that asked for this one, </span>
        run {delegation.parent_run_id.slice(0, 8)} →
      </button>
    </p>
  );
}

/**
 * Runs another run asked for, beneath the call that asked.
 *
 * Depth is drawn as indentation and a branch from the row above, and said in
 * words to a screen reader, which hears each row's asker before its agent.
 * The taint mark is the one every run carries: a child's taint is its
 * parent's, the tracker being one.
 */
export function Delegations({
  rows,
  navigate,
}: {
  rows: readonly DelegationRow[];
  navigate: Navigate;
}) {
  return (
    <div className="trace-delegations" role="list" aria-label="Work asked of other agents">
      {rows.map((row) => (
        <div
          role="listitem"
          key={row.key}
          className="trace-delegation"
          style={{ "--delegation-depth": row.depth } as CSSProperties}
        >
          <span className="visually-hidden">asked by {row.askedBy}: </span>
          <span className="trace-delegation-agent">
            <Icon name="agents" size={14} />
            <strong>{row.agent}</strong>
          </span>
          <span className={`verdict ${row.tone}`}>
            <span className="visually-hidden">delegation </span>
            {sentence(row.state)}
          </span>
          <span className="muted">{row.steps === 1 ? "1 step" : `${row.steps} steps`}</span>
          {row.tainted ? <Tainted /> : null}
          <span className="muted prose trace-delegation-objective">
            <Visible text={row.objective} />
          </span>
          {row.runId !== null ? (
            <button
              type="button"
              className="ghost trace-delegation-open"
              onClick={() => {
                if (row.runId !== null) navigate({ name: "tasks", runId: row.runId });
              }}
            >
              <span className="visually-hidden">{row.agent}'s </span>trace →
            </button>
          ) : null}
        </div>
      ))}
    </div>
  );
}
