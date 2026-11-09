import type { EventView } from "../../bindings/EventView";
import { Icon } from "../../components/common";
import { clock } from "../../sdk/format";
import { canOpen } from "./activityModel";
import { eventIcon, eventTone } from "./eventFamilies";

/** The id of the record section a row opens, for `aria-controls`. */
export const RECORD_ID = "activity-record";

/**
 * One line of the feed.
 *
 * A stored event is a button that opens its record. A streamed one is not
 * yet in the log by id, so it is plain text rather than a control that would
 * answer with an error.
 *
 * The node on the rail carries the event's family as an icon and its tone as
 * a hue; a security-relevant event also has a bar along its leading edge and
 * its kind in the attention hue. The hidden prefix is the same fact for
 * everyone else, since colour carries nothing to a screen reader.
 */
export function FeedRow({
  event,
  open,
  arrived,
  onOpen,
}: {
  event: EventView;
  open: boolean;
  /** It came in since the feed was last drawn, and rises into place. */
  arrived: boolean;
  onOpen: () => void;
}) {
  const tone = eventTone(event);
  const security = event.security_relevant;
  const openable = canOpen(event);
  const classes = [
    "feed-row",
    `tone-${tone}`,
    security ? "security" : null,
    arrived ? "arrived" : null,
    openable ? null : "pending",
  ]
    .filter(Boolean)
    .join(" ");

  const content = (
    <>
      {security ? <span className="feed-mark" aria-hidden="true" /> : null}
      <span className="feed-time">{clock(event.at)}</span>
      <span className="feed-node" aria-hidden="true">
        <Icon name={eventIcon(event.kind)} size={14} />
      </span>
      {security ? <span className="visually-hidden">security relevant: </span> : null}
      <span className="feed-kind">{event.kind}</span>
      <span className="feed-summary" title={event.summary}>
        {event.summary}
      </span>
      <span className="feed-trail" aria-hidden="true">
        <Icon name={openable ? "chevron-right" : "clock"} size={14} />
      </span>
    </>
  );

  if (!openable) {
    return (
      <div className={`row ${classes}`} title="Still being written to the chain">
        {content}
      </div>
    );
  }
  return (
    <button
      type="button"
      className={`row clickable ${classes}`}
      data-spotlight
      data-event-id={event.id}
      aria-expanded={open}
      aria-controls={open ? RECORD_ID : undefined}
      onClick={onOpen}
    >
      {content}
    </button>
  );
}
