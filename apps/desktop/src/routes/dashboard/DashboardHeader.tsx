import { Icon, LiveDot, PageHeader } from "../../components/common";
import { useEvents } from "../../sdk/eventStream";
import { usingFixtures } from "../../sdk/transport";
import type { Navigate } from "../route";
import type { Tile } from "./dashboardModel";
import { type StatusPart, greeting, statusLine } from "./overviewModel";

/**
 * The dashboard's title block: a greeting by the hour over the title, the
 * screen's one primary action beside it, and a status line beneath that says
 * whether the system can be trusted right now.
 *
 * The primary action follows what the screen most needs: while anything waits
 * on a decision it is the way to those decisions, and otherwise it is the way
 * to give an agent work.
 */
export function DashboardHeader({
  now,
  chain,
  missing,
  waiting,
  navigate,
}: {
  now: number;
  chain: Tile;
  /** Setup checks still missing, or `null` before setup has been read. */
  missing: number | null;
  /** Approvals waiting on a person. */
  waiting: number;
  navigate: Navigate;
}) {
  const at = new Date(now);
  const day = at.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });

  const action =
    waiting > 0 ? (
      <button
        type="button"
        className="primary dash-action"
        onClick={() => navigate({ name: "approvals" })}
      >
        <Icon name="approvals" size={16} />
        Review {waiting} waiting
      </button>
    ) : (
      <button
        type="button"
        className="primary dash-action"
        onClick={() => navigate({ name: "tasks" })}
      >
        <Icon name="plus" size={16} />
        New task
      </button>
    );

  return (
    <PageHeader
      eyebrow={
        <>
          {greeting(at.getHours())}
          <span aria-hidden="true"> · </span>
          <span className="visually-hidden">, </span>
          {day}
        </>
      }
      title="Dashboard"
      subtitle={<StatusLine chain={chain} missing={missing} />}
      actions={action}
    />
  );
}

/**
 * The status line. Its own component because it follows the event feed, which
 * changes with every event, and only this line should redraw when it does.
 */
function StatusLine({ chain, missing }: { chain: Tile; missing: number | null }) {
  const feedFailing = useEvents().error !== null;
  const parts = statusLine({ fixtures: usingFixtures(), feedFailing, chain, missing });
  return (
    <span className="dash-status">
      {parts.map((part) => (
        <StatusFact key={part.id} part={part} />
      ))}
    </span>
  );
}

function StatusFact({ part }: { part: StatusPart }) {
  return (
    <span className={`dash-status-part tone-${part.tone}`}>
      {part.mark === "live" ? <LiveDot tone="ok" /> : <Icon name={part.mark} size={14} />}
      {part.text}
    </span>
  );
}
