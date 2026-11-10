import type { AgentSummary } from "../../bindings/AgentSummary";
import type { PolicyView } from "../../bindings/PolicyView";
import type { TaskSummary } from "../../bindings/TaskSummary";
import { CopyButton, Enabled, LiveDot, Risk, StatTile } from "../../components/common";
import { ago } from "../../sdk/format";
import { taskTally } from "./detailModel";
import { type GrantRow, type ReportState, grantTally, refusedFigure } from "./grantsModel";
import { jumpToSection } from "./hooks";
import { Section } from "./Section";

/**
 * What an agent is at a glance: four figures that each lead to the section
 * behind them, its instructions, and the facts that identify it.
 *
 * The refused-grants figure is the one that may glow: a grant every call of
 * which will be refused is an agent that looks more capable than it is, and
 * that is worth noticing before anything else on the page.
 */
export function Overview({
  summary,
  instructions,
  workspace,
  policy,
  rows,
  report,
  tasks,
}: {
  summary: AgentSummary;
  instructions: string;
  workspace: string;
  policy: PolicyView | null;
  rows: readonly GrantRow[];
  /** Where the engine's grant report stands; only a read one can rule refusals out. */
  report: ReportState;
  tasks: readonly TaskSummary[];
}) {
  const grants = grantTally(rows);
  const refused = refusedFigure(rows, report);
  const work = taskTally(tasks);

  return (
    <Section id="agent-overview" title="Overview" icon="info">
      <div className="grid stats overview-stats">
        <StatTile
          label="Tools granted"
          icon="zap"
          value={grants.granted}
          hint={
            grants.highest !== null ? (
              <>
                <span aria-hidden="true">up to </span>
                <span className="visually-hidden">highest granted </span>
                <Risk level={grants.highest} />
              </>
            ) : undefined
          }
          onClick={() => jumpToSection("agent-tools")}
        />
        <StatTile
          label="Refused grants"
          icon="lock"
          value={refused.value}
          tone={refused.tone}
          hint={refused.hint}
          onClick={() => jumpToSection("agent-tools")}
        />
        <StatTile
          label="Policy rules"
          icon="shield"
          value={policy ? policy.rules.length : "none"}
          tone={policy ? "accent" : "warn"}
          hint={
            policy
              ? `version ${policy.version} · default ${policy.default_effect}`
              : "every action is denied"
          }
          onClick={() => jumpToSection("agent-policy")}
        />
        <StatTile
          label="Recent tasks"
          icon="tasks"
          value={work.total}
          hint={
            work.running > 0 ? (
              <span className="hint-live">
                <LiveDot tone="info" />
                {work.running} running
              </span>
            ) : (
              "none running"
            )
          }
          onClick={() => jumpToSection("agent-recent")}
        />
      </div>

      <div className="overview-grid">
        <div className="panel overview-card">
          <h3 className="card-label">Instructions</h3>
          <div className="prose overview-instructions">{instructions}</div>
        </div>
        <div className="panel overview-card overview-facts">
          <h3 className="card-label">Details</h3>
          <dl className="facts flush">
            <dt>Status</dt>
            <dd>
              <Enabled status={summary.status} />
            </dd>
            <dt>Model</dt>
            <dd className="mono">
              {summary.provider}/{summary.model}
            </dd>
            <dt>Steps per run</dt>
            <dd>{summary.max_steps}</dd>
            <dt>Created</dt>
            <dd>{ago(summary.created_at)}</dd>
            <dt>Workspace</dt>
            <dd>
              <span className="mono">{workspace}</span>
              <CopyButton text={workspace} label="Copy the workspace path" />
            </dd>
            <dt>Identity</dt>
            <dd>
              <span className="mono faint">{summary.id}</span>
              <CopyButton text={summary.id} label="Copy the agent id" />
            </dd>
          </dl>
        </div>
      </div>
    </Section>
  );
}
