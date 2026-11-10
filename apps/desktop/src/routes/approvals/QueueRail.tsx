import { Icon, Kbd, Risk, Tainted } from "../../components/common";
import { riskTone } from "../../components/status";
import { type Intent, blockedFor } from "./approvalsModel";
import { type DeskEntry, stampOf } from "./decisionModel";
import { queueItemLabel, toolIcon } from "./deskModel";

/**
 * The queue beside the presented request, drawn only when the window is wide
 * enough to hold both (the stylesheet decides; in the stacked layouts the
 * cards are the queue).
 *
 * Each item is a button that presents its request, when pressed or focused.
 * It does not decide anything, and it does not move the keyboard into the
 * card: the operator reads the queue here, and Tab carries them on to the
 * request when they are ready. An item holding a half-made decision says so,
 * so a decision set aside on one request is not forgotten while reading
 * another; and so does one whose answer failed, because the reason is on its
 * card and that card may not be the one presented.
 *
 * The queue is one stop for Tab, on the presented item, and j, k and the
 * arrows move within it. Otherwise every request waiting would stand between
 * the operator and the buttons of the one they are reading.
 */
export function QueueRail({
  entries,
  waiting,
  shown,
  intent,
  failures,
  now,
  cardsId,
  registerItem,
  onSelect,
}: {
  entries: readonly DeskEntry[];
  waiting: number;
  shown: string | null;
  intent: Intent | null;
  /** Why the last answer to a request failed, by its id. */
  failures: ReadonlyMap<string, string>;
  now: number;
  /** The id of the cards column the items present into. */
  cardsId: string;
  registerItem: (id: string, element: HTMLElement | null) => void;
  onSelect: (id: string) => void;
}) {
  return (
    // Marked for the keyboard handler; see QUEUE_RAIL in useDesk.
    <section className="approvals-rail" data-queue-rail="" aria-labelledby="approvals-rail-title">
      <div className="rail-head">
        <h2 id="approvals-rail-title">Waiting on you</h2>
        <span className="rail-count">{waiting}</span>
      </div>
      <ul className="rail-list stagger" role="list">
        {entries.map((entry) => {
          if (entry.kind === "ghost") {
            const { approval, phase } = entry.departure;
            const stamp = stampOf(entry.departure);
            return (
              <li
                key={`leaving-${approval.id}`}
                className={`rail-ghost ${phase} ${stamp.tone}`}
                aria-hidden="true"
              >
                <div className="ghost-fold">
                  <div className="queue-item departing">
                    <span className="queue-item-top">
                      <span className="queue-item-agent">{approval.agent_name}</span>
                      <span className="queue-item-stamp">
                        <Icon name={stamp.icon} size={13} />
                        {stamp.words}
                      </span>
                    </span>
                    <span className="queue-item-tool mono">{approval.tool}</span>
                  </div>
                </div>
              </li>
            );
          }
          const { approval } = entry;
          const armed = intent?.id === approval.id ? intent : null;
          const failed = failures.has(approval.id);
          const label = queueItemLabel(approval, now, { armed: armed !== null, failed });
          return (
            <li key={approval.id}>
              <button
                type="button"
                ref={(element) => registerItem(approval.id, element)}
                className={`queue-item lift risk-${riskTone(approval.risk)}`}
                data-spotlight
                tabIndex={shown === approval.id ? 0 : -1}
                aria-current={shown === approval.id ? "true" : undefined}
                aria-controls={cardsId}
                aria-label={label}
                onClick={() => onSelect(approval.id)}
                // As a card does: the keyboard's place in the queue is the
                // selection, so the arrows move on from the item focus is on.
                onFocus={() => onSelect(approval.id)}
              >
                <span className="queue-item-top">
                  <span className="queue-item-agent">
                    <Icon name={toolIcon(approval.tool)} size={14} />
                    {approval.agent_name}
                  </span>
                  <Risk level={approval.risk} />
                </span>
                <span className="queue-item-tool mono">{approval.tool}</span>
                <span className="queue-item-meta">
                  <span className="queue-item-age">
                    <Icon name="clock" size={12} />
                    {blockedFor(approval.requested_at, now)}
                  </span>
                  {approval.tainted ? <Tainted label="untrusted" /> : null}
                  {armed === null ? null : (
                    <span
                      className={`queue-item-armed ${armed.kind === "approve" ? "approving" : "denying"}`}
                    >
                      {armed.kind === "approve" ? "approving" : "denying"}
                    </span>
                  )}
                  {failed ? (
                    <span className="queue-item-failed">
                      <Icon name="alert" size={12} />
                      not answered
                    </span>
                  ) : null}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {/* The keys, said beside the queue they move through. The stacked
          layouts say the same above the cards instead; the stylesheet shows
          one or the other, never both. */}
      <div className="rail-foot">
        <Icon name="shield" size={14} />
        <p>
          <Kbd>j</Kbd> <Kbd>k</Kbd> or <Kbd>↓</Kbd> <Kbd>↑</Kbd> move between requests, most
          dangerous first. <Kbd>Esc</Kbd> sets a half-made decision down. No key approves or denies;
          the buttons do, and each asks you to confirm.
        </p>
      </div>
    </section>
  );
}
