import { Icon } from "../../components/common";
import { unseenLabel } from "./feedSignals";

/**
 * What sits after the feed: the way back to the newest event, the sentence
 * that says what the window is a window onto, and the notice for events still
 * being written to the chain.
 */
export function FeedFooter({
  unseen,
  onJump,
  note,
  unwritten,
  onReadAgain,
}: {
  /** Events that came in since Follow was switched off. */
  unseen: number;
  /** Switch Follow back on, which jumps to the newest event. */
  onJump: () => void;
  note: string | null;
  /** Events shown that cannot be opened yet. */
  unwritten: number;
  onReadAgain: () => void;
}) {
  return (
    <>
      {unseen > 0 ? (
        <div className="feed-jump-dock">
          <button type="button" className="feed-jump" onClick={onJump}>
            <Icon name="chevron-down" size={14} />
            {unseenLabel(unseen)}
          </button>
        </div>
      ) : null}

      {note ? <p className="field-note activity-note">{note}</p> : null}
      {unwritten > 0 ? (
        <p className="field-note activity-note">
          {unwritten === 1 ? "One event" : `${unwritten} events`} arrived after history was read
          and {unwritten === 1 ? "is" : "are"} still being written to the chain, so{" "}
          {unwritten === 1 ? "it opens" : "they open"} once history is read again.{" "}
          <button type="button" className="ghost" onClick={onReadAgain}>
            Read history again
          </button>
        </p>
      ) : null}
    </>
  );
}
