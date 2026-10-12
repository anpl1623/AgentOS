import { useEffect, useId, useRef, useState } from "react";

import { setLeaveConfirmer } from "../sdk/drafts";
import { useModal } from "../shell/modal";

/**
 * A yes-or-no question asked inside the window.
 *
 * Focus starts on the answer that changes nothing, so a person who presses
 * Enter without reading keeps what they had. Escape and the backdrop give the
 * same answer.
 */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel,
  onAnswer,
}: {
  title: string;
  /** Paragraphs separated by a blank line. */
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  onAnswer: (confirmed: boolean) => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const bodyId = useId();
  useModal(dialog, () => onAnswer(false), cancel);

  return (
    <div
      className="palette-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onAnswer(false);
      }}
    >
      <div
        ref={dialog}
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        tabIndex={-1}
      >
        <div className="dialog-title" id={titleId}>
          {title}
        </div>
        <div id={bodyId}>
          {message.split(/\n{2,}/).map((paragraph, index) => (
            <p key={index}>{paragraph}</p>
          ))}
        </div>
        <div className="dialog-actions">
          <button type="button" ref={cancel} className="primary" onClick={() => onAnswer(false)}>
            {cancelLabel}
          </button>
          <button type="button" className="danger" onClick={() => onAnswer(true)}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

interface Question {
  message: string;
  answer: (leave: boolean) => void;
}

/**
 * The navigation guard's way of asking, installed for as long as it is mounted.
 *
 * Tauri's webview is not guaranteed to show `window.confirm`, and a prompt that
 * never appears is a question nobody can answer; so the shell asks here. A new
 * question while one is open answers the old one "stay", because only the
 * latest attempt to leave is still wanted.
 */
export function LeaveConfirmerHost() {
  const [question, setQuestion] = useState<Question | null>(null);
  const open = useRef<Question | null>(null);

  useEffect(() => {
    setLeaveConfirmer(
      (message) =>
        new Promise<boolean>((resolve) => {
          open.current?.answer(false);
          const next = { message, answer: resolve };
          open.current = next;
          setQuestion(next);
        }),
    );
    return () => {
      setLeaveConfirmer(null);
      open.current?.answer(false);
      open.current = null;
    };
  }, []);

  if (question === null) return null;
  return (
    <ConfirmDialog
      title="Leave without saving?"
      message={question.message}
      confirmLabel="Leave anyway"
      cancelLabel="Stay"
      onAnswer={(leave) => {
        question.answer(leave);
        if (open.current === question) {
          open.current = null;
          setQuestion(null);
        }
      }}
    />
  );
}
