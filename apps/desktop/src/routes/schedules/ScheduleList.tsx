import type { ScheduleView } from "../../bindings/ScheduleView";
import {
  EmptyState,
  ErrorBanner,
  Icon,
  Loading,
  SkeletonRows,
  Stale,
} from "../../components/common";
import { ScheduleCard } from "./ScheduleCard";
import type { PendingChange } from "./useScheduleActions";

/**
 * The standing schedules, as a grid of cards in the order they fire.
 *
 * A failed refresh keeps the cards on screen and says the data is old; a
 * failed first read says it could not be read at all. An empty list is an
 * answer, said with what a schedule is and the one action that makes one.
 *
 * The heading takes focus after a delete, so the person is left at the list
 * rather than nowhere once the card they deleted from has gone.
 */
export function ScheduleList({
  list,
  loading,
  error,
  staleSince,
  actionError,
  now,
  running,
  busy,
  fresh,
  onPause,
  onDelete,
  onLastTask,
  onCreate,
}: {
  /** Sorted; `null` until the first read lands. */
  list: readonly ScheduleView[] | null;
  loading: boolean;
  error: string | null;
  /** When the data on screen was read, if the latest refresh failed. */
  staleSince: number | null;
  actionError: string | null;
  now: number;
  running: boolean | null;
  /** The schedule a change is in flight for, and which change. */
  busy: PendingChange | null;
  /** The schedule just created. */
  fresh: string | null;
  onPause: (schedule: ScheduleView, paused: boolean) => void;
  onDelete: (schedule: ScheduleView) => void;
  onLastTask: () => void;
  onCreate: () => void;
}) {
  return (
    <section className="schedule-section" aria-labelledby="schedules-list">
      <div className="schedule-section-head">
        <h2 id="schedules-list" tabIndex={-1}>
          Standing
        </h2>
        {staleSince !== null ? <Stale since={staleSince} /> : null}
      </div>

      {error ? (
        <ErrorBanner
          message={
            list
              ? `The schedules could not be re-read: ${error}`
              : `The schedules could not be read: ${error}`
          }
        />
      ) : null}
      {actionError ? <ErrorBanner message={actionError} /> : null}

      {loading ? (
        <div className="panel">
          <Loading what="schedules" />
          <SkeletonRows count={2} />
        </div>
      ) : null}

      {list?.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon="calendar"
            title="No schedules yet"
            action={
              <button type="button" className="primary" onClick={onCreate}>
                <Icon name="plus" size={16} />
                Create a schedule
              </button>
            }
          >
            <p>
              A schedule gives an agent the same objective on an interval, on a cron, or once. It
              fires only while AgentOS is open with its scheduler on, or while{" "}
              <code className="mono">agentos schedule run</code> runs.
            </p>
          </EmptyState>
        </div>
      ) : null}

      {list && list.length > 0 ? (
        <div className="schedule-grid stagger" role="list" aria-labelledby="schedules-list">
          {list.map((schedule) => (
            <div role="listitem" key={schedule.id}>
              <ScheduleCard
                schedule={schedule}
                now={now}
                running={running}
                change={busy?.id === schedule.id ? busy.change : null}
                locked={busy !== null}
                fresh={fresh === schedule.id}
                onPause={(paused) => onPause(schedule, paused)}
                onDelete={() => onDelete(schedule)}
                onLastTask={onLastTask}
              />
            </div>
          ))}
        </div>
      ) : null}
    </section>
  );
}
