import type { ReactNode, RefObject } from "react";

import { Loading, SkeletonRows } from "../../components/common";
import { type DayGroup, dayLabel } from "./activityModel";
import { FeedRow } from "./FeedRow";

/**
 * The feed: a log region of day groups, each with a separator that pins
 * beneath the page header while its events scroll past.
 *
 * The region speaks while Follow is on and nothing has hushed it; see
 * `useHush` for why a deliberate change to its rows is made in silence. The
 * marker at its end is what Follow keeps in view and measures from.
 */
export function Feed({
  feedRef,
  endRef,
  label,
  speaking,
  loading,
  loadingWhat,
  empty,
  groups,
  today,
  openId,
  arrived,
  onToggle,
}: {
  feedRef: RefObject<HTMLDivElement | null>;
  endRef: RefObject<HTMLDivElement | null>;
  label: string;
  /** The region announces what it gains. */
  speaking: boolean;
  /** Nothing to show yet, and a read is under way. */
  loading: boolean;
  loadingWhat: string;
  /** What to draw when there is nothing to show and nothing coming. */
  empty: ReactNode;
  groups: readonly DayGroup[];
  today: Date;
  openId: string | null;
  arrived: ReadonlySet<string>;
  onToggle: (id: string) => void;
}) {
  return (
    <div
      ref={feedRef}
      className="panel activity-feed"
      role="log"
      tabIndex={-1}
      aria-label={label}
      aria-live={speaking ? "polite" : "off"}
    >
      {loading ? (
        <div className="activity-loading">
          <Loading what={loadingWhat} />
          <SkeletonRows count={4} />
        </div>
      ) : null}
      {empty}
      {groups.map((group) => {
        const day = dayLabel(group.day, today);
        return (
          <section key={group.id} className="feed-group" aria-label={day}>
            {/* The count is for the eye: it changes with every arrival, and a
             * log region would speak each change. */}
            <div className="feed-day">
              <h2 className="feed-day-label">{day}</h2>
              <span className="feed-day-count" aria-hidden="true">
                {group.events.length === 1 ? "1 event" : `${group.events.length} events`}
              </span>
            </div>
            {/* Roles rather than ul and li: the semantics are the same to
             * assistive technology, and the elements bring indents and
             * bullets that no stylesheet here resets. */}
            <div role="list" className="feed-list">
              {group.events.map((event) => (
                <div role="listitem" key={event.id}>
                  <FeedRow
                    event={event}
                    open={openId === event.id}
                    arrived={arrived.has(event.id)}
                    onOpen={() => onToggle(event.id)}
                  />
                </div>
              ))}
            </div>
          </section>
        );
      })}
      <div ref={endRef} />
    </div>
  );
}
