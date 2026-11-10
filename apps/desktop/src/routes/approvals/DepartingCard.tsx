import { Icon } from "../../components/common";
import { riskTone } from "../../components/status";
import { CardBody, CardHead } from "./CardParts";
import { type Departure, stampOf } from "./decisionModel";

/**
 * An answered card on its way out: the same card, stamped with a check or a
 * cross, then folded shut so the cards after it rise into its place. The stamp
 * says only what the runtime has said (see `stampOf`).
 *
 * Only a picture. The store took the request out of the queue on the confirm
 * and the status line has already said what was done, so this is hidden from
 * assistive technology and inert: nothing on it can be pressed, focused or
 * read twice, and nothing waits for it to finish.
 */
export function DepartingCard({ departure, now }: { departure: Departure; now: number }) {
  const { approval, phase } = departure;
  const stamp = stampOf(departure);
  const prefix = `leaving-${approval.id}`;
  return (
    <div className={`approval-slot leaving ${phase}`} aria-hidden="true" inert>
      <div className="ghost-fold">
        <div className={`approval departing ${stamp.tone} risk-${riskTone(approval.risk)}`}>
          <CardHead approval={approval} now={now} titleId={`${prefix}-title`} />
          <div className="approval-body">
            <CardBody approval={approval} idPrefix={prefix} stop={false} failure={null} />
          </div>
          {/* A pane of glass over the card, and the answer in its foot. The
              foot is sticky, as the live card's was, so the stamp shows
              where the confirm was pressed however the card was scrolled. */}
          <div className="approval-stamp" />
          <div className="approval-foot stamped">
            <span className="stamp-mark">
              <Icon name={stamp.icon} size={20} />
            </span>
            <span>{stamp.words}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
