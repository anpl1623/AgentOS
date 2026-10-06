import { Fragment, useId, useRef } from "react";

import { shortcutLines } from "../shell/keys";
import { useModal } from "../shell/modal";

/**
 * The shortcut sheet, opened with `?`.
 *
 * The accelerators are documented here and in the sidebar footer's tooltip
 * rather than as visible chrome: a person who wants them asks, and a person
 * who does not is not made to read them on every screen.
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
        <div className="dialog-title" id={titleId}>
          Keyboard shortcuts
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
                        <kbd>{key}</kbd>
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
