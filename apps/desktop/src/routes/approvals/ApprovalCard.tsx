import { useEffect, useRef } from "react";

import type { ApprovalView } from "../../bindings/ApprovalView";
import { riskTone } from "../../components/status";
import { type Intent, type IntentKind, offersStop } from "./approvalsModel";
import { CardBody, CardHead } from "./CardParts";
import { DecisionBar } from "./DecisionBar";

/**
 * One request waiting on a person.
 *
 * Everything a decision needs is on the card, because an approval that sends
 * someone hunting through other screens is an approval that gets clicked
 * without being read. The card is bounded to the window and its foot is
 * sticky, so however large the arguments are, the decision stays in reach.
 *
 * Its edge carries the risk's hue. Only the card presented as the one to read
 * next carries the glow, so a long queue reads as one lit card among quiet
 * ones rather than a wall of amber.
 */
export function ApprovalCard({
  approval,
  now,
  intent,
  shown,
  selected,
  linked,
  failure,
  register,
  onSelect,
  onIntend,
  onNote,
  onCancel,
  onConfirm,
  onDrill,
}: {
  approval: ApprovalView;
  now: number;
  /** The half-made decision, when it is this card's. */
  intent: Intent | null;
  /** The card presented as the one to read next; see `shownId`. */
  shown: boolean;
  selected: boolean;
  /** The card a link sent the operator to. */
  linked: boolean;
  /** Why the last answer to this card failed, if it did. */
  failure: string | null;
  register: (id: string, element: HTMLElement | null) => void;
  onSelect: () => void;
  onIntend: (kind: IntentKind) => void;
  onNote: (text: string) => void;
  onCancel: () => void;
  onConfirm: (intent: Intent) => void;
  onDrill: () => void;
}) {
  const article = useRef<HTMLElement>(null);
  const { id } = approval;
  const prefix = `approval-${id}`;
  const titleId = `${prefix}-title`;
  const stop = offersStop(approval);

  useEffect(() => {
    register(id, article.current);
    return () => register(id, null);
  }, [id, register]);

  const className = [
    "approval",
    `risk-${riskTone(approval.risk)}`,
    shown ? "shown" : "",
    selected ? "selected" : "",
    selected && linked ? "focused" : "",
    intent === null ? "" : "armed",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article
      ref={article}
      className={className}
      aria-labelledby={titleId}
      aria-describedby={approval.tainted ? `${prefix}-taint` : undefined}
      tabIndex={-1}
      onFocus={onSelect}
    >
      <CardHead approval={approval} now={now} titleId={titleId} onDrill={onDrill} />

      {/* Bounded and scrollable, so it takes a tab stop: a keyboard user must
          be able to scroll to everything they are being asked to allow. */}
      <div
        className="approval-body"
        tabIndex={0}
        role="group"
        aria-label="What it is asking to do"
        data-scroll-region=""
      >
        <CardBody approval={approval} idPrefix={prefix} stop={stop} failure={failure} />
      </div>

      <DecisionBar
        id={id}
        stop={stop}
        intent={intent}
        onIntend={onIntend}
        onNote={onNote}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />
    </article>
  );
}
