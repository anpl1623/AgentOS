import type { AgentSummary } from "../../bindings/AgentSummary";
import type { AuditHealth } from "../../bindings/AuditHealth";
import type { TaskSummary } from "../../bindings/TaskSummary";
import {
  EmptyState,
  Enabled,
  Icon,
  LiveDot,
  Row,
  SkeletonRows,
  Stale,
} from "../../components/common";
import type { LiveTone } from "../../components/LiveDot";
import { ago } from "../../sdk/format";
import type { Navigate } from "../route";
import type { Tile } from "./dashboardModel";
import { type AgentActivity, agentActivity, auditSummary } from "./overviewModel";
import { Item, List, Section, SeeAll } from "./Section";

/** Placeholder rows on first paint. */
const SKELETON_ROWS = 3;

/**
 * The audit chain: whether it verifies, how many records it holds, and when
 * it was last checked.
 *
 * The state and its hue come from the chain tile, which decides what outranks
 * what; the card only lays it out. A check that failed keeps the last answer
 * on screen, marked stale, rather than letting it pass as current.
 */
export function AuditCard({
  chain,
  health,
  stale,
  unrecorded,
  navigate,
}: {
  chain: Tile;
  health: AuditHealth | null;
  stale: boolean;
  /** The sentence about records never written, or `null`. */
  unrecorded: string | null;
  navigate: Navigate;
}) {
  const tone = chain.tone ?? "neutral";
  const summary = auditSummary(chain, health ? health.events : null);
  return (
    <Section
      id="dashboard-audit"
      title="Audit chain"
      icon="shield"
      tail={
        <SeeAll
          label="Verify"
          where="in Settings"
          onActivate={() => navigate({ name: "settings" })}
        />
      }
    >
      <div className={`panel dash-audit tone-${tone}`}>
        <div className="dash-audit-main">
          <span className="dash-audit-mark" aria-hidden="true">
            <Icon name="shield" size={22} />
          </span>
          <div className="dash-audit-figures">
            <span className="dash-audit-state">
              <span className="visually-hidden">Audit chain: </span>
              {summary.state}
            </span>
            <span className="dash-audit-events">{summary.detail}</span>
          </div>
        </div>
        {unrecorded ? <p className="dash-audit-warning">{unrecorded}</p> : null}
        {health ? (
          <p className="dash-audit-note">
            The chain is checked in full once per launch, then as records are written; Settings
            rehashes all of it.{" "}
            {stale ? (
              <Stale since={health.checked_at} />
            ) : (
              <>Last checked {ago(health.checked_at)}.</>
            )}
          </p>
        ) : null}
      </div>
    </Section>
  );
}

/**
 * How an agent's work in the running list is marked beside its name: the dot
 * pulses only for a run that is moving, holds still in the attention hue for
 * one waiting on a decision, and holds still for a task not yet started — the
 * same reading the running cards give the same runs.
 */
const ACTIVITY: Readonly<
  Record<AgentActivity, { tone: LiveTone; pulse: boolean; words: string }>
> = {
  working: { tone: "info", pulse: true, words: "working" },
  waiting: { tone: "warn", pulse: false, words: "waiting on you" },
  queued: { tone: "info", pulse: false, words: "queued" },
};

/**
 * Every configured agent, with a dot beside the ones that have work in the
 * running list, so a glance says who is busy and who is held up.
 */
export function Agents({
  agents,
  running,
  first,
  navigate,
}: {
  agents: readonly AgentSummary[] | null;
  running: readonly TaskSummary[];
  first: boolean;
  navigate: Navigate;
}) {
  const activity = agentActivity(running);
  return (
    <Section
      id="dashboard-agents"
      title="Agents"
      icon="agents"
      count={agents && agents.length > 0 ? agents.length : null}
      tail={
        agents && agents.length > 0 ? (
          <SeeAll label="Manage" where="agents" onActivate={() => navigate({ name: "agents" })} />
        ) : null
      }
    >
      <div className="panel">
        {first ? (
          <SkeletonRows count={SKELETON_ROWS} />
        ) : !agents ? null : agents.length === 0 ? (
          <EmptyState
            icon="agents"
            title="No agents yet."
            action={
              // The visible words are the whole name, so a person who says
              // what they see to a voice control is understood.
              <button type="button" className="primary" onClick={() => navigate({ name: "agents" })}>
                Create an agent
              </button>
            }
          >
            An agent is a model with tools and a policy.
          </EmptyState>
        ) : (
          <List labelledBy="dashboard-agents" className="stagger">
            {agents.map((agent) => (
              <Item key={agent.id}>
                <Row
                  className="dash-agent"
                  onActivate={() => navigate({ name: "agents", agent: agent.name })}
                >
                  <div className="row-main">
                    <div className="row-title dash-agent-name">
                      {agent.name}
                      <AgentBusy activity={activity.get(agent.id)} />
                    </div>
                    <div className="row-meta">
                      <span className="mono">
                        {agent.provider}/{agent.model}
                      </span>
                      <span>
                        {agent.tools.length === 1 ? "1 tool" : `${agent.tools.length} tools`}
                      </span>
                    </div>
                  </div>
                  <Enabled status={agent.status} />
                </Row>
              </Item>
            ))}
          </List>
        )}
      </div>
    </Section>
  );
}

/** The mark beside an agent with work in the running list; nothing for one without. */
function AgentBusy({ activity }: { activity: AgentActivity | undefined }) {
  if (activity === undefined) return null;
  const { tone, pulse, words } = ACTIVITY[activity];
  return (
    <span className={`dash-agent-busy ${activity}`}>
      <LiveDot tone={tone} pulse={pulse} />
      {words}
    </span>
  );
}
