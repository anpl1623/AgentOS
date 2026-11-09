import type { RefObject } from "react";

import { Icon } from "../../components/common";
import { ago } from "../../sdk/format";
import { DONE, RUN_CLAUSE, type Receipt, type StatusTone } from "./decisionModel";

/**
 * The record of this visit: the status line, which says what is being done
 * and then what was, and a receipt for each decision delivered while the
 * screen has been open.
 *
 * The status line is a live region present from the first render, so its
 * first message is spoken, and it takes focus when a decision is confirmed:
 * the card that took the decided one's place has not been read, and focus
 * left on its buttons would be one press from deciding it.
 */
export function DecisionLog({
  status,
  tone,
  statusRef,
  receipts,
}: {
  status: string;
  tone: StatusTone;
  statusRef: RefObject<HTMLDivElement | null>;
  receipts: readonly Receipt[];
}) {
  return (
    <div className="decision-log">
      <div
        className={`decided decision-status ${tone}`}
        role="status"
        tabIndex={-1}
        ref={statusRef}
      >
        {status}
      </div>
      {receipts.length > 0 ? (
        <ul className="receipts" role="list" aria-label="Decided while this screen was open">
          {receipts.map((receipt) => (
            <li
              className={`decided receipt ${receipt.kind === "approve" ? "approved" : "denied"}`}
              // A request whose answer did not stick can be answered again.
              key={`${receipt.id}@${receipt.at}`}
            >
              <span className="receipt-mark" aria-hidden="true">
                <Icon name={receipt.kind === "approve" ? "check" : "x"} size={12} />
              </span>
              <span>
                {DONE[receipt.kind]} <span className="mono">{receipt.tool}</span> for{" "}
                {receipt.agent}
                {receipt.run === undefined ? null : RUN_CLAUSE[receipt.run]}
                <span className="faint">
                  {" · "}
                  {ago(receipt.at)}
                </span>
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
