import { Icon } from "../../components/common";
import { riskTone } from "../../components/status";
import { CardBody, CardHead } from "./CardParts";
import { type Departure, STAMP } from "./decisionModel";

/**
 * An answered card on its way out: the same card, stamped with a check or a
 * cross, then folded shut so the cards after it rise into its place.
 *
 * Only a picture. The store took the request out of the queue on the confirm
 * and the status line has already said what was done, so this is hidden from
 * assistive technology and inert: nothing on it can be pressed, focused or
 * read twice, and nothing waits for it to finish.
 */
export function DepartingCard({ departure, now }: { departure: Departure; now: number }) {
  const { approval, kind, phase } = departure;
  const tone = kind === "approve" ? "approved" : "denied";
  const prefix = `leaving-${approval.id}`;
  return (
    <div className={`approval-slot leaving ${phase}`} aria-hidden="true" inert>
      <div className="ghost-fold">
        <div className={`approval departing ${tone} risk-${riskTone(approval.risk)}`}>
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
              <Icon name={kind === "approve" ? "check" : "x"} size={20} />
            </span>
            <span>{STAMP[kind]}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
