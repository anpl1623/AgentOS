import { useCallback, useState } from "react";

import {
  ErrorBanner,
  Loading,
  PageHeader,
  SkeletonRows,
  Stale,
  Switch,
} from "../../components/common";
import { cacheKey, useAsyncCached } from "../../sdk/cache";
import { api, describeError } from "../../sdk/client";
import { useAsync } from "../../sdk/useAsync";
import type { Navigate } from "../route";
import { AgentAvatar } from "./AgentAvatar";
import { DETAIL_SECTIONS } from "./detailModel";
import { grantRows } from "./grantsModel";
import { GrantsSection } from "./GrantsSection";
import { useActiveSection, useLoadedAt } from "./hooks";
import { MemoryPanel } from "./MemoryPanel";
import { Overview } from "./Overview";
import { PolicyEditor } from "./PolicyEditor";
import { RecentTasks } from "./RecentTasks";
import { Section } from "./Section";
import { SectionNav } from "./SectionNav";

/**
 * One agent: who it is and whether it may be given work, then its overview,
 * the tools it holds and how far its policy lets them reach, the policy
 * itself, what it remembers, and what it has done lately. On a wide window the
 * sections are listed beside the page, marking the one being read.
 */
export function AgentDetail({ name, navigate }: { name: string; navigate: Navigate }) {
  const agent = useAsyncCached(cacheKey("get_agent", name), () => api.getAgent(name), [name]);
  const catalogue = useAsyncCached(cacheKey("list_tools"), () => api.listTools());
  const id = agent.data?.summary.id ?? null;
  // Not cached: the report is the engine's reading of the policy as stored
  // now, and an answer carried over from an earlier visit could describe a
  // policy that has since been replaced.
  const grants = useAsync(() => (id === null ? Promise.resolve(null) : api.grantReport(id)), [id]);

  const loadedAt = useLoadedAt(agent.data);
  const active = useActiveSection(DETAIL_SECTIONS, agent.data !== null);

  const [toggling, setToggling] = useState(false);
  const [toggleError, setToggleError] = useState<string | null>(null);

  const reloadAgent = agent.reload;
  const reloadGrants = grants.reload;
  const onPolicySaved = useCallback(() => {
    reloadAgent();
    reloadGrants();
  }, [reloadAgent, reloadGrants]);

  const parent = { label: "Agents", onActivate: () => navigate({ name: "agents" }) };

  if (agent.data === null) {
    return (
      <div className="agents-screen">
        <PageHeader eyebrow="Agent" title={name} parent={parent} />
        {agent.error ? (
          <ErrorBanner message={agent.error} />
        ) : (
          <>
            <Loading what={name} />
            <div className="panel">
              <SkeletonRows count={3} />
            </div>
          </>
        )}
      </div>
    );
  }

  const { summary, instructions, policy, recent_tasks, workspace } = agent.data;
  const rows = grantRows(summary.tools, catalogue.data, grants.data, policy);
  const enabled = summary.status === "enabled";

  const toggle = async () => {
    setToggling(true);
    setToggleError(null);
    try {
      await api.setAgentEnabled(summary.name, !enabled);
      agent.reload();
    } catch (failure) {
      setToggleError(describeError(failure));
    } finally {
      setToggling(false);
    }
  };

  return (
    <div className="agents-screen">
      <PageHeader
        eyebrow="Agent"
        parent={parent}
        leading={<AgentAvatar name={summary.name} enabled={enabled} size="sm" />}
        title={summary.name}
        subtitle={
          <>
            {summary.provider}/{summary.model} · {summary.max_steps} steps per run
            {agent.stale ? (
              <>
                {" · "}
                <Stale since={loadedAt} />
              </>
            ) : null}
          </>
        }
        actions={
          <Switch
            checked={enabled}
            disabled={toggling}
            label="Enabled"
            onChange={() => void toggle()}
          />
        }
      />
      {agent.error ? <ErrorBanner message={agent.error} /> : null}
      {toggleError ? <ErrorBanner message={toggleError} /> : null}

      <div className="agent-detail">
        <SectionNav sections={DETAIL_SECTIONS} active={active} />
        <div className="agent-sections">
          <Overview
            summary={summary}
            instructions={instructions}
            workspace={workspace}
            policy={policy}
            rows={rows}
            report={grants.data !== null ? "read" : grants.error !== null ? "failed" : "reading"}
            tasks={recent_tasks}
          />
          <GrantsSection
            rows={rows}
            policy={policy}
            reportError={grants.error}
            catalogueError={catalogue.error}
          />
          <Section id="agent-policy" title="Policy" icon="shield">
            <PolicyEditor
              key={summary.id}
              agentId={summary.id}
              policy={policy}
              onSaved={onPolicySaved}
            />
          </Section>
          <MemoryPanel agentId={summary.id} />
          <RecentTasks tasks={recent_tasks} navigate={navigate} />
        </div>
      </div>
    </div>
  );
}
