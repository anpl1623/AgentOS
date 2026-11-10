import type { SchedulerView } from "../../bindings/SchedulerView";
import { ErrorBanner, Icon, LiveDot, StatTile } from "../../components/common";
import {
  type ScheduleSummary,
  countHints,
  overdueNote,
  schedulerFacts,
  soonestTile,
} from "./overviewModel";

/**
 * Whether anything on this screen is firing.
 *
 * The scheduler in this window is the only one the screen can see. Another,
 * started with `agentos schedule run`, would fire these schedules with this
 * window's switch off, so the warning says "unless" rather than claiming
 * nothing fires anywhere. A status that could not be read is said to be
 * unknown, not assumed in either direction.
 *
 * The switch itself lives in Settings beside the tick and the run limit it
 * starts with; this card says where it is rather than keeping a second copy.
 */
export function SchedulerCard({
  status,
  error,
  now,
  openSettings,
}: {
  status: SchedulerView | null;
  error: string | null;
  now: number;
  openSettings: () => void;
}) {
  if (status === null) {
    if (error) {
      return (
        <ErrorBanner
          message={`Whether the scheduler is running could not be read, so this screen cannot say whether these schedules fire: ${error}`}
        />
      );
    }
    // Holds the card's place while the first read is out, so the list does
    // not jump down when it lands. It is hidden from assistive technology:
    // the list's own loading line is what speaks while the screen is read.
    return <div className="scheduler-card is-pending skeleton" aria-hidden="true" />;
  }

  if (status.running) {
    return (
      <section className="scheduler-card is-running" aria-labelledby="scheduler-state">
        <span className="scheduler-mark" aria-hidden="true">
          <LiveDot tone="ok" />
        </span>
        <div className="scheduler-body">
          <p className="scheduler-title" id="scheduler-state">
            Scheduler running
          </p>
          <ul className="scheduler-facts" aria-label="Scheduler">
            {schedulerFacts(status, now).map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
          <p className="scheduler-text">
            Schedules fire only while AgentOS is open, or while{" "}
            <code className="mono">agentos schedule run</code> runs.
          </p>
        </div>
        <div className="scheduler-action">
          <button type="button" className="ghost" onClick={openSettings}>
            <Icon name="settings" size={16} />
            Scheduler settings
          </button>
        </div>
      </section>
    );
  }

  const overdue = overdueNote(status.overdue);
  return (
    <section className="scheduler-card is-off" aria-labelledby="scheduler-state">
      <span className="scheduler-mark" aria-hidden="true">
        <Icon name="pause" size={18} />
      </span>
      <div className="scheduler-body">
        <p className="scheduler-title" id="scheduler-state">
          Scheduler off
        </p>
        <p className="scheduler-text">
          These schedules are kept, and fire only while AgentOS is open with its scheduler on, or
          while <code className="mono">agentos schedule run</code> runs elsewhere on this machine.
        </p>
        {status.error ? (
          <p className="scheduler-text scheduler-error">It stopped on its own: {status.error}</p>
        ) : null}
        {overdue ? <p className="scheduler-text">{overdue}</p> : null}
      </div>
      <div className="scheduler-action">
        <button type="button" onClick={openSettings}>
          Turn it on in Settings
          <Icon name="chevron-right" size={14} />
        </button>
      </div>
    </section>
  );
}

/**
 * The figures above the list, counted the way the cards read.
 *
 * Overdue takes the attention hue only when something is overdue, and the
 * soonest firing says so when the scheduler is off: a countdown to a moment
 * nothing will act on must not read as a promise.
 */
export function ScheduleStats({
  summary,
  running,
}: {
  /** `null` while the list is first read; the tiles hold their places. */
  summary: ScheduleSummary | null;
  running: boolean | null;
}) {
  const next = summary === null ? null : soonestTile(summary, running);
  const hints = summary === null ? null : countHints(summary, running);
  const overdue = summary !== null && summary.overdue > 0;
  return (
    <div className="grid stats schedule-stats">
      <StatTile label="Active" value={summary?.active ?? null} icon="play" hint={hints?.active} />
      <StatTile label="Paused" value={summary?.paused ?? null} icon="pause" hint={hints?.paused} />
      <StatTile
        label="Overdue"
        value={summary?.overdue ?? null}
        icon="alert"
        tone={overdue ? "warn" : "accent"}
        hint={hints?.overdue}
      />
      <StatTile label="Next firing" value={next?.value ?? null} icon="clock" hint={next?.hint} />
    </div>
  );
}
