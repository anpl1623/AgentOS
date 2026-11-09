import { type MouseEvent, useEffect, useRef } from "react";

import { Icon, Kbd } from "../../components/common";
import { type Intent, type IntentKind, NOTE_LIMIT, isDeliberate, noteCount } from "./approvalsModel";

/** A click that counts, for the decision buttons; see `isDeliberate`. */
function deliberate(event: MouseEvent): boolean {
  return isDeliberate(event.detail);
}

/**
 * The foot of an approval card: Deny, Deny and stop the run, and Approve; and,
 * once one of them is pressed, the note and the press that confirms it.
 *
 * Deny on the left and Approve past the spacer, as everywhere a
 * deny-by-default product asks. Both take two presses: an interface must not
 * make allowing an action cheaper than refusing it. The confirm is drawn where
 * the first press landed, so `isDeliberate` turns away the rest of a double
 * click rather than letting it confirm.
 */
export function DecisionBar({
  id,
  stop,
  intent,
  onIntend,
  onNote,
  onCancel,
  onConfirm,
}: {
  /** The request's id, for element ids. */
  id: string;
  /** Whether to offer denying and stopping the run together. */
  stop: boolean;
  /** The half-made decision, when it is this card's. */
  intent: Intent | null;
  onIntend: (kind: IntentKind) => void;
  onNote: (text: string) => void;
  onCancel: () => void;
  onConfirm: (intent: Intent) => void;
}) {
  const note = useRef<HTMLInputElement>(null);
  const buttons = useRef(new Map<IntentKind, HTMLButtonElement>());
  const kind = intent?.kind ?? null;
  const noteId = `approval-${id}-note`;
  const helpId = `approval-${id}-note-help`;

  // Arming a decision opens the note and puts the keyboard in it. A second
  // press of the same key therefore types into the note; it does not confirm.
  // Setting the decision down puts the keyboard back on the button that armed
  // it, unless focus has already gone somewhere on purpose. The note opens in
  // the sticky foot, which is in view wherever the card is scrolled, so the
  // move does not scroll: scrolling to the field would carry the card's title,
  // the thing being decided, up under the pinned page header.
  const armed = useRef<IntentKind | null>(null);
  useEffect(() => {
    const was = armed.current;
    armed.current = kind;
    if (kind !== null) {
      note.current?.focus({ preventScroll: true });
      return;
    }
    const active = document.activeElement;
    if (was !== null && (active === null || active === document.body)) {
      buttons.current.get(was)?.focus();
    }
  }, [kind]);

  const keep = (which: IntentKind) => (element: HTMLButtonElement | null) => {
    if (element === null) buttons.current.delete(which);
    else buttons.current.set(which, element);
  };

  if (intent === null) {
    return (
      <div className="approval-foot">
        <button
          type="button"
          className="danger decision"
          ref={keep("deny")}
          onClick={(event) => deliberate(event) && onIntend("deny")}
        >
          <Icon name="x" size={16} />
          Deny
        </button>
        {stop ? (
          <button
            type="button"
            className="danger decision"
            ref={keep("stop")}
            onClick={(event) => deliberate(event) && onIntend("stop")}
          >
            <Icon name="stop" size={16} />
            Deny and stop the run
          </button>
        ) : null}
        <span className="spacer" />
        <button
          type="button"
          className="primary decision"
          ref={keep("approve")}
          onClick={(event) => deliberate(event) && onIntend("approve")}
        >
          <Icon name="check" size={16} />
          Approve
        </button>
      </div>
    );
  }

  const approving = intent.kind === "approve";
  const count = noteCount(intent.note);
  const cancel = (
    <button type="button" className="ghost decision-cancel" onClick={onCancel} aria-keyshortcuts="Escape">
      Cancel
      <Kbd hidden>Esc</Kbd>
    </button>
  );
  return (
    <div className={approving ? "approval-foot armed approving" : "approval-foot armed denying"}>
      <div className="decision-note">
        <label htmlFor={noteId}>
          {approving ? "Why are you allowing this?" : "Why are you declining?"}
        </label>
        <input
          id={noteId}
          ref={note}
          value={intent.note}
          maxLength={NOTE_LIMIT}
          aria-describedby={helpId}
          onChange={(event) => onNote(event.target.value)}
        />
        <div className="decision-note-foot">
          <p className="field-note" id={helpId}>
            {approving
              ? "Optional. Kept with your decision in the audit log."
              : "Optional. Kept with your decision in the audit log, and given to the agent as the reason."}
          </p>
          <span className={count.near ? "note-count near" : "note-count"} aria-hidden="true">
            {count.line}
          </span>
        </div>
      </div>
      <div className="decision-buttons">
        {approving ? (
          <>
            <span className="spacer" />
            {cancel}
            <button
              type="button"
              className="primary decision"
              onClick={(event) => deliberate(event) && onConfirm(intent)}
            >
              <Icon name="check" size={16} />
              Confirm approve
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="danger decision"
              onClick={(event) => deliberate(event) && onConfirm(intent)}
            >
              <Icon name={intent.kind === "stop" ? "stop" : "x"} size={16} />
              {intent.kind === "stop" ? "Confirm deny and stop" : "Confirm deny"}
            </button>
            {cancel}
            <span className="spacer" />
          </>
        )}
      </div>
    </div>
  );
}
