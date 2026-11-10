import {
  ErrorBanner,
  Icon,
  Loading,
  PageHeader,
  SkeletonRows,
} from "../../components/common";
import { useNow } from "../../components/hooks";
import { cacheKey, useAsyncCached } from "../../sdk/cache";
import { api } from "../../sdk/client";
import { useAsync } from "../../sdk/useAsync";
import type { Navigate } from "../route";
import { Appearance } from "./Appearance";
import { AuditLog } from "./AuditLog";
import { type AuditState, useAuditVerification, useScheduler } from "./hooks";
import { Integrations } from "./Integrations";
import { Browser, Storage } from "./Machine";
import { NetworkCredentials } from "./NetworkCredentials";
import { Overview } from "./Overview";
import { overviewTiles } from "./overviewModel";
import { Providers } from "./Providers";
import { Scheduler } from "./Scheduler";
import { SectionNav } from "./SectionNav";
import { visibleSections } from "./sectionsModel";
import { Tools } from "./Tools";
import { useSectionSpy } from "./useSectionNav";

/**
 * The machine-level controls, and the one question only this screen answers:
 * who on this machine has been granted each tool.
 *
 * A long page, so it opens with the four facts an operator would otherwise
 * scroll for, and keeps a navigation of its sections beside them (above them,
 * where the window is narrow) that marks the section being read. The sections
 * are in the order `sectionsModel.ts` gives and explains.
 *
 * The title row does not pin as it does on other screens: here the section
 * navigation is what stays in reach, and two sticky bars would take a third
 * of a short window between them.
 */
export function Settings(_props: { navigate?: Navigate | undefined }) {
  const settings = useAsyncCached(cacheKey("settings"), () => api.settings());
  const agents = useAsyncCached(cacheKey("list_agents"), () => api.listAgents());
  const integrations = useAsync(() => api.listIntegrations(), []);
  const scheduler = useScheduler();
  const audit = useAuditVerification();
  const now = useNow();
  const data = settings.data;

  // Nothing until the first answer, so the panels that load on their own do
  // not paint first and then slide down under the ones that waited.
  const waiting = data === null && settings.loading;
  const sections = visibleSections(data !== null);
  const { active, jump } = useSectionSpy(waiting ? [] : sections.map((entry) => entry.id));
  const keychain = data?.keychain_available ?? null;

  const header = <Header audit={audit} />;
  if (waiting) {
    return (
      <>
        {header}
        <Loading what="settings" />
        <div className="panel">
          <SkeletonRows count={4} />
        </div>
      </>
    );
  }

  return (
    <>
      {header}

      {/* Content whenever there is some; the bare banner only when there is
          nothing to show beneath it. */}
      {settings.error ? (
        <ErrorBanner
          message={
            data ? `Settings could not be refreshed: ${settings.error}` : settings.error
          }
        />
      ) : null}

      <Overview
        tiles={overviewTiles({
          settings: data,
          integrations: integrations.data,
          scheduler: scheduler.view,
          audit: audit.last,
          checking: audit.verifying,
          checkError: audit.error,
          now,
        })}
        onJump={jump}
      />

      <div className="settings-layout">
        <SectionNav sections={sections} active={active} onJump={jump} />
        <div className="settings-sections">
          <Appearance />
          {data ? <Providers settings={data} onChanged={settings.reload} /> : null}
          <Integrations listed={integrations} keychain={keychain} />
          <NetworkCredentials keychain={keychain} />
          {data ? <Browser settings={data} /> : null}
          <Scheduler scheduler={scheduler} />
          <AuditLog audit={audit} />
          {data ? (
            <Tools
              tools={data.tools}
              agents={agents.data}
              agentsError={agents.data === null ? agents.error : null}
            />
          ) : null}
          {data ? <Storage settings={data} /> : null}
        </div>
      </div>
    </>
  );
}

/**
 * The title, and the screen's one primary action: checking the audit chain,
 * the deliberate act of trust this screen exists for. Its answer lands in the
 * overview tile just below, so pressing it never needs a scroll to see what
 * it found; the audit section has the details.
 */
function Header({ audit }: { audit: AuditState }) {
  return (
    <PageHeader
      eyebrow="This machine"
      title="Settings"
      subtitle="Everything AgentOS knows lives on this machine. Nothing here is uploaded anywhere."
      sticky={false}
      actions={
        <button
          type="button"
          className="primary settings-button"
          aria-disabled={audit.verifying}
          onClick={audit.verify}
        >
          <Icon name="shield" size={16} />
          {audit.verifying ? "Verifying…" : "Verify audit chain"}
        </button>
      }
    />
  );
}
