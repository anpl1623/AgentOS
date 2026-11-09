import { Fragment, useId, useRef } from "react";

import { shortcutLines } from "../shell/keys";
import { useModal } from "../shell/modal";
import { Icon } from "./icons";
import { Kbd } from "./Kbd";

/**
 * The shortcut sheet, opened with `?`.
 *
 * The accelerators are documented here rather than spelled out across the
 * chrome: a person who wants them asks — with `?`, or the keyboard button at
 * the sidebar's foot — and a person who does not is not made to read them on
 * every screen. The palette launcher shows its one shortcut beside it.
 */
export function ShortcutSheet({ apple, onClose }: { apple: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useModal(dialog, onClose);

  return (
    <div
      className="palette-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialog}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <div className="dialog-head">
          <span className="dialog-mark" aria-hidden="true">
            <Icon name="command" size={18} />
          </span>
          <div className="dialog-title" id={titleId}>
            Keyboard shortcuts
          </div>
        </div>
        <dl>
          {shortcutLines(apple).map((line) => (
            <Fragment key={line.does}>
              <dt>
                {line.keys.map((combo, index) => (
                  <Fragment key={combo.join("+")}>
                    {index > 0 ? <span className="faint"> or </span> : null}
                    {combo.map((key, at) => (
                      <Fragment key={key}>
                        {at > 0 && !apple ? "+" : null}
                        <Kbd>{key}</Kbd>
                      </Fragment>
                    ))}
                  </Fragment>
                ))}
              </dt>
              <dd>{line.does}</dd>
            </Fragment>
          ))}
        </dl>
        <p className="field-note">
          Nothing here answers an approval. Approving costs a deliberate click, on the card.
        </p>
        <div className="dialog-actions">
          <button type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
