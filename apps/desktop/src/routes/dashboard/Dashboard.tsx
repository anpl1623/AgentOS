import { ErrorBanner } from "../../components/common";
import { useNow } from "../../components/hooks";
import type { Navigate } from "../route";
import { DashboardHeader } from "./DashboardHeader";
import { chainTile, unrecordedNote } from "./dashboardModel";
import { KpiRow } from "./KpiRow";
import { NeedsYou } from "./NeedsYou";
import { Failed, Refused } from "./Outcomes";
import { Running } from "./Running";
import { SetupCard } from "./SetupCard";
import { AuditCard, Agents } from "./SideCards";
import { ToolUsage } from "./ToolUsage";
import { useDashboardData } from "./useDashboardData";

/**
 * What is happening right now.
 *
 * Ordered by what would make someone act: the figures that say whether to
 * look anywhere else, then anything setup still lacks, then the things waiting
 * on the person, then what is running, then what went wrong or was refused.
 * On a wide window the trust and inventory cards — the audit chain, the tools
 * and the agents — take a column of their own beside that; on a narrow one
 * they follow it.
 */
export function Dashboard({ navigate }: { navigate: Navigate }) {
  const now = useNow();
  const {
    view,
    data,
    first,
    pending,
    health,
    days,
    setDays,
    usage,
    today,
    refusals,
    settings,
    missing,
  } = useDashboardData();

  const chain = chainTile(health.data, health.stale, health.error !== null);

  return (
    <>
      <DashboardHeader
        now={now}
        chain={chain}
        missing={settings.data && data ? missing.length : null}
        waiting={pending.loaded ? pending.approvals.length : 0}
        navigate={navigate}
      />
      {first ? (
        <p className="visually-hidden" role="status">
          Loading the dashboard…
        </p>
      ) : null}

      <div className="dash-banners">
        {view.error ? <ErrorBanner message={view.error} /> : null}
        {health.error ? (
          <ErrorBanner message={`The audit chain could not be checked: ${health.error}`} />
        ) : null}
        {health.data && !health.data.intact ? (
          <div className="banner error">
            The audit log does not verify. Its contents are unreliable from the first break onwards
            — see Settings.
          </div>
        ) : null}
        {settings.error ? (
          <ErrorBanner
            message={`This installation's setup could not be checked: ${settings.error}`}
          />
        ) : null}
      </div>

      <KpiRow
        now={now}
        approvals={pending.approvals}
        approvalsKnown={pending.loaded}
        approvalsFailed={pending.error !== null}
        running={data ? data.running_tasks : null}
        today={today.data}
        todayFailed={today.error !== null}
        usage={usage.data}
        navigate={navigate}
      />

      {missing.length > 0 ? <SetupCard missing={missing} navigate={navigate} /> : null}

      {/* The container the two-column layout measures; see dashboard.css. */}
      <div className="dash-body">
        <div className="dash-layout">
          <div className="dash-main">
            <NeedsYou pending={pending} now={now} navigate={navigate} />
            <Running
              tasks={data ? data.running_tasks : null}
              agents={data?.agents ?? []}
              first={first}
              navigate={navigate}
            />
            <Failed tasks={data ? data.recent_failures : null} first={first} navigate={navigate} />
            <Refused refusals={refusals} loaded={data !== null} first={first} navigate={navigate} />
          </div>
          <div className="dash-side">
            <AuditCard
              chain={chain}
              health={health.data}
              stale={health.stale}
              unrecorded={unrecordedNote(health.data)}
              navigate={navigate}
            />
            <ToolUsage
              usage={usage.data}
              loading={usage.loading}
              error={usage.error}
              days={days}
              setDays={setDays}
            />
            <Agents
              agents={data ? data.agents : null}
              running={data?.running_tasks ?? []}
              first={first}
              navigate={navigate}
            />
          </div>
        </div>
      </div>
    </>
  );
}
