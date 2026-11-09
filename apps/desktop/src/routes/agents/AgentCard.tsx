import type { AgentSummary } from "../../bindings/AgentSummary";
import { Enabled, Icon, Risk } from "../../components/common";
import { ago } from "../../sdk/format";
import { AgentAvatar } from "./AgentAvatar";
import { domainIcon, toolCountLabel, toolDomains } from "./agentsModel";

/**
 * One agent in the list, as a card that opens it.
 *
 * The whole card is one real button — Enter, Space, focus and the
 * announcement come from the platform — so everything inside it is plain
 * text: no link or control may nest in a button. It lifts and lights under
 * the pointer like every other card that does something when pressed.
 *
 * The risk chip is the most dangerous baseline risk among the tools the agent
 * holds, before its policy narrows anything; it is left off until the
 * catalogue has answered rather than drawn as a guess.
 */
export function AgentCard({
  agent,
  highest,
  onOpen,
}: {
  agent: AgentSummary;
  /** The highest granted risk, or `null` while unknown or when nothing known is granted. */
  highest: string | null;
  onOpen: () => void;
}) {
  const enabled = agent.status === "enabled";
  const domains = toolDomains(agent.tools);
  return (
    <button
      type="button"
      className={enabled ? "agent-card lift" : "agent-card lift off"}
      data-spotlight
      onClick={onOpen}
    >
      <span className="agent-card-head">
        <AgentAvatar name={agent.name} enabled={enabled} />
        <span className="agent-card-id">
          <span className="agent-card-name">{agent.name}</span>
          <span className="agent-card-model">
            {agent.provider}/{agent.model}
          </span>
        </span>
        <Enabled status={agent.status} />
      </span>

      {domains.length > 0 ? (
        <span className="agent-card-domains">
          <span className="visually-hidden">tools from </span>
          {domains.map((domain) => (
            <span key={domain} className="agent-domain">
              <Icon name={domainIcon(domain)} size={13} />
              {domain}
            </span>
          ))}
        </span>
      ) : null}

      <span className="agent-card-foot">
        <span className="agent-card-fact">
          <Icon name="zap" size={14} />
          {toolCountLabel(agent.tools.length)}
        </span>
        {highest !== null ? (
          <span className="agent-card-fact">
            <span aria-hidden="true">up to</span>
            <span className="visually-hidden">highest granted</span> <Risk level={highest} />
          </span>
        ) : null}
        <span className="agent-card-fact faint">created {ago(agent.created_at)}</span>
        <Icon name="chevron-right" size={16} className="agent-card-go" />
      </span>
    </button>
  );
}
