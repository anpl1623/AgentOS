import { type CSSProperties, useEffect, useRef, useState } from "react";

import {
  EmptyState,
  ErrorBanner,
  Icon,
  Loading,
  PageHeader,
  StatTile,
} from "../../components/common";
import { cacheKey, useAsyncCached } from "../../sdk/cache";
import { api } from "../../sdk/client";
import { useDraft, useUnsavedGuard } from "../../sdk/drafts";
import type { Navigate } from "../route";
import { AgentCard } from "./AgentCard";
import {
  STATUS_FILTERS,
  type StatusFilter,
  agentStats,
  filterAgents,
  highestRisk,
  matchesStatus,
} from "./agentsModel";
import { CreateAgent } from "./CreateAgent";
import { CREATE_GUARD, type CreateForm, createFormDirty, newCreateForm } from "./createModel";
import { Item, List } from "./lists";

/**
 * Every agent, as a grid of cards, with the figures that matter over them and
 * the form that makes a new one.
 */
export function AgentList({ navigate }: { navigate: Navigate }) {
  const agents = useAsyncCached(cacheKey("list_agents"), () => api.listAgents());
  const catalogue = useAsyncCached(cacheKey("list_tools"), () => api.listTools());
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const newButton = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);

  // The open form is the draft: `null` is closed. One object, so one guard
  // covers every field of it.
  const [form, setForm, discardForm] = useDraft<CreateForm | null>("agent:new", null);
  useUnsavedGuard(createFormDirty(form), CREATE_GUARD);
  // Leaving was either confirmed against the guard's "discard it?" or had
  // nothing to lose, so the draft goes with the screen. Kept, it would come
  // back on the next visit as a form the operator was told was thrown away.
  useEffect(() => discardForm, [discardForm]);

  // Navigating to the new agent waits for the commit that discards the form,
  // because the guard is unregistered by an effect; navigating from the click
  // handler would ask whether to discard a form that has just been created.
  const [created, setCreated] = useState<string | null>(null);
  useEffect(() => {
    if (created !== null) navigate({ name: "agents", agent: created });
  }, [created, navigate]);

  const open = () => setForm(newCreateForm());
  // Closing from inside the form would drop focus to the page when its
  // buttons unmount; the button that opened it is where the operator was.
  const close = () => {
    discardForm();
    newButton.current?.focus();
  };

  const all = agents.data ?? [];
  const shown = filterAgents(all, query).filter((agent) => matchesStatus(agent, status));
  const stats = agents.data ? agentStats(agents.data, catalogue.data) : null;
  const filtering = query.trim() !== "" || status !== "all";

  return (
    <div className="agents-screen">
      <PageHeader
        eyebrow="Workforce"
        title="Agents"
        subtitle="Each agent has its own instructions, its own workspace, and its own policy."
        actions={
          <button
            ref={newButton}
            type="button"
            // The screen's primary action until the form is open; then it
            // only closes the form, and Create inside it is the primary one.
            className={form === null ? "primary" : undefined}
            aria-expanded={form !== null}
            onClick={() => (form === null ? open() : discardForm())}
          >
            <Icon name={form === null ? "plus" : "close"} size={16} />
            {form === null ? "New agent" : "Cancel"}
          </button>
        }
      />

      {form !== null ? (
        <CreateAgent
          form={form}
          update={setForm}
          agents={agents.data}
          catalogue={catalogue.data}
          catalogueError={catalogue.error}
          onCancel={close}
          onCreated={(name) => {
            discardForm();
            agents.reload();
            setCreated(name);
          }}
        />
      ) : null}

      {agents.error ? <ErrorBanner message={agents.error} /> : null}

      <div className="grid stats agents-stats">
        <StatTile
          label="Agents"
          icon="agents"
          value={stats?.total ?? null}
          hint={
            stats && stats.total > 0
              ? stats.providers === 1
                ? "on 1 provider"
                : `across ${stats.providers} providers`
              : undefined
          }
        />
        <StatTile
          label="Enabled"
          icon="check"
          value={stats?.enabled ?? null}
          hint={stats ? `${stats.disabled} disabled` : undefined}
        />
        <StatTile
          label="Tools in use"
          icon="zap"
          value={stats?.toolsInUse ?? null}
          hint={catalogue.data ? `of ${catalogue.data.length} in the catalogue` : undefined}
        />
        <StatTile
          label="High-risk agents"
          icon="alert"
          value={stats?.highRisk ?? null}
          tone={stats?.highRisk ? "warn" : "accent"}
          hint="hold a high or critical tool"
        />
      </div>

      <h2 id="agents-list" className="visually-hidden">
        All agents
      </h2>
      <div className="toolbar agents-toolbar">
        <div className="agents-search">
          <Icon name="search" size={16} />
          <input
            ref={search}
            type="search"
            aria-label="Filter agents by name, provider or model"
            placeholder="Filter by name, provider or model"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <div className="segmented" role="group" aria-label="Status">
          {STATUS_FILTERS.map((choice) => (
            <button
              key={choice.value}
              type="button"
              aria-pressed={status === choice.value}
              onClick={() => setStatus(choice.value)}
            >
              {choice.label}
            </button>
          ))}
        </div>
        {agents.data && agents.data.length > 0 ? (
          <span className="agents-count">
            {filtering ? `${shown.length} of ${all.length}` : `${all.length} agents`}
          </span>
        ) : null}
      </div>

      {agents.loading ? (
        <>
          <Loading what="agents" />
          <CardSkeletons count={3} />
        </>
      ) : null}

      {agents.data?.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon="agents"
            title="No agents yet"
            action={
              form === null ? (
                <button type="button" className="primary" onClick={open}>
                  <Icon name="plus" size={16} />
                  Create an agent
                </button>
              ) : undefined
            }
          >
            An agent is a model with instructions, the tools it is offered, and a policy that
            decides each call.
          </EmptyState>
        </div>
      ) : null}

      {agents.data && agents.data.length > 0 && shown.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon="search"
            title={
              query.trim() !== "" ? `No agent matches “${query.trim()}”` : "No agent matches"
            }
            action={
              <button
                type="button"
                onClick={() => {
                  setQuery("");
                  setStatus("all");
                  // This button leaves with the empty state it sits in; the
                  // filter it just cleared is where the operator was working.
                  search.current?.focus();
                }}
              >
                Show every agent
              </button>
            }
          >
            {status !== "all" ? `Showing ${status} agents only.` : undefined}
          </EmptyState>
        </div>
      ) : null}

      {shown.length > 0 ? (
        <List labelledBy="agents-list" className="agent-grid stagger">
          {shown.map((agent) => (
            <Item key={agent.id}>
              <AgentCard
                agent={agent}
                highest={catalogue.data ? highestRisk(agent.tools, catalogue.data) : null}
                onOpen={() => navigate({ name: "agents", agent: agent.name })}
              />
            </Item>
          ))}
        </List>
      ) : null}
    </div>
  );
}

// Varied so a grid of placeholders reads as cards rather than a pattern.
const SKELETON_WIDTHS = [48, 36, 56] as const;

/**
 * Placeholder cards in the grid's shape. Hidden from assistive technology:
 * the `Loading` beside them is what speaks.
 */
function CardSkeletons({ count }: { count: number }) {
  return (
    <div className="agent-grid" aria-hidden="true">
      {Array.from({ length: count }, (_, index) => (
        <div className="agent-card skeleton-card" key={index}>
          <span className="skeleton agent-avatar md" />
          <span
            className="skeleton-line"
            style={{ "--w": `${SKELETON_WIDTHS[index % SKELETON_WIDTHS.length]}%` } as CSSProperties}
          />
          <span className="skeleton-line" style={{ "--w": "30%" } as CSSProperties} />
        </div>
      ))}
    </div>
  );
}
