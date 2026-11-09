import { useId, useRef, useState } from "react";

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { ToolView } from "../../bindings/ToolView";
import { EmptyState, ErrorBanner, Icon, Risk } from "../../components/common";
import {
  type CatalogueEntry,
  catalogue,
  catalogueSummary,
  countEntries,
  grantLine,
} from "./catalogueModel";
import { Item, List, Section } from "./parts";
import { counted } from "./settingsModel";

/**
 * Every tool, by domain, with who holds it.
 *
 * The Agents screen answers what one agent may do; this answers the reverse,
 * which is the question an operator brings here: who on this machine can run
 * `terminal.exec`?
 */
export function Tools({
  tools,
  agents,
  agentsError,
}: {
  tools: readonly ToolView[];
  agents: readonly AgentSummary[] | null;
  agentsError: string | null;
}) {
  const filterId = useId();
  const [filter, setFilter] = useState("");
  const field = useRef<HTMLInputElement>(null);
  const groups = catalogue(tools, agents, filter);
  const shown = countEntries(groups);
  const filtering = filter.trim() !== "";
  const summary = catalogueSummary(tools);

  return (
    <Section
      id="settings-tools"
      intro={
        <>
          Every tool an agent can be granted, with who holds it.{" "}
          <span className="verdict warn">external</span> marks a tool whose output someone other
          than you can write. Reading it raises the approval bar for the rest of the run.
        </>
      }
    >
      {agentsError ? (
        <ErrorBanner message={`Who holds each tool could not be read: ${agentsError}`} />
      ) : null}
      <div className="toolbar settings-toolbar">
        <div className="settings-search">
          <Icon name="search" size={16} />
          <input
            ref={field}
            id={filterId}
            type="search"
            aria-label="Filter tools"
            placeholder="Filter by tool, capability or agent"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
        </div>
        {/* Not live: the status line below announces a filter's count, and
            this is read where it stands, with the figures nothing else says. */}
        <span className="settings-tool-count">
          {filtering ? `${shown} of ${tools.length} shown` : counted(tools.length, "tool", "tools")}
          {" · "}
          {summary.dangerous} high or critical · {summary.external} external
        </span>
      </div>
      <p className="visually-hidden" role="status">
        {filtering ? `${shown} of ${tools.length} tools shown.` : ""}
      </p>
      {groups.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon={tools.length === 0 ? "zap" : "filter"}
            title={tools.length === 0 ? "No tools are registered." : "No tool matches the filter."}
            action={
              filtering ? (
                <button
                  type="button"
                  onClick={() => {
                    setFilter("");
                    // The button goes with the empty state; focus goes back
                    // to the field rather than to the top of the window.
                    field.current?.focus();
                  }}
                >
                  Clear the filter
                </button>
              ) : undefined
            }
          >
            {tools.length === 0
              ? "This runtime offers agents nothing to call."
              : "A term matches a tool's name, risk, capability, or an agent that holds it."}
          </EmptyState>
        </div>
      ) : (
        <div className="settings-stack">
          {groups.map((group) => {
            const headingId = `${filterId}-${group.domain}`;
            return (
              <section key={group.domain} className="panel" aria-labelledby={headingId}>
                <div className="settings-domain-head">
                  <h3 id={headingId} className="mono">
                    {group.domain}
                  </h3>
                  <span className="faint">
                    {counted(group.entries.length, "tool", "tools")}
                  </span>
                </div>
                <List labelledBy={headingId}>
                  {group.entries.map((entry) => (
                    <Item key={entry.tool.name}>
                      <ToolRow entry={entry} />
                    </Item>
                  ))}
                </List>
              </section>
            );
          })}
        </div>
      )}
    </Section>
  );
}

function ToolRow({ entry }: { entry: CatalogueEntry }) {
  const { tool } = entry;
  const holders = grantLine(entry.holders);
  return (
    <div className="row settings-tool">
      <div className="row-main">
        <div className="row-title mono">{tool.name}</div>
        <div className="row-meta">
          <span>{tool.description}</span>
        </div>
        <div className="settings-tool-foot">
          {tool.capabilities.length > 0 ? (
            <span className="settings-tags">
              <span className="visually-hidden">Capabilities:</span>
              {tool.capabilities.map((capability) => (
                <span key={capability} className="tag">
                  {capability}
                </span>
              ))}
            </span>
          ) : null}
          <span className={holders.nobody ? "settings-holders faint" : "settings-holders"}>
            <Icon name="agents" size={12} />
            {holders.text}
          </span>
        </div>
      </div>
      <div className="settings-tool-chips">
        <Risk level={tool.risk} />
        {tool.returns_untrusted_data ? (
          <span className="verdict warn">
            external
            <span className="visually-hidden">
              . Its output can be written by someone other than you, so reading it raises the
              approval bar for the rest of the run.
            </span>
          </span>
        ) : null}
      </div>
    </div>
  );
}
