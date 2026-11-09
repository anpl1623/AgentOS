import { useCopy } from "./hooks";
import { Icon } from "./icons";

/** How long the check stays after a copy, before the button is ready again. */
const CONFIRM_MS = 1_500;

/**
 * Copies `text` to the clipboard, and shows and says that it did.
 *
 * The text is sanitised on the way (see `useCopy`): what lands on the
 * clipboard is what a person could read in the window, with no escape
 * sequence riding along into a terminal. The copy icon turns into a check for
 * a moment, or the alert mark if the webview refused; the same outcome is
 * spoken politely, from a region that is always present so its first change
 * is heard.
 *
 * `label` names what is copied — "Copy the run id" — and is the button's name
 * and tooltip. With `showLabel` the words are drawn too; otherwise it is an
 * icon button.
 */
export function CopyButton({
  text,
  label = "Copy",
  showLabel = false,
}: {
  text: string;
  label?: string | undefined;
  showLabel?: boolean | undefined;
}) {
  const { copy, state } = useCopy(CONFIRM_MS);
  return (
    <>
      <button
        type="button"
        className={showLabel ? "copy-button labelled" : "copy-button icon-button"}
        data-state={state}
        aria-label={showLabel ? undefined : label}
        data-tip={showLabel ? undefined : label}
        onClick={() => void copy(text)}
      >
        <span className="copy-glyphs" aria-hidden="true">
          <Icon name="copy" size={16} className="copy-glyph-idle" />
          <Icon name="check" size={16} className="copy-glyph-done" />
          <Icon name="alert" size={16} className="copy-glyph-failed" />
        </span>
        {showLabel ? <span>{label}</span> : null}
      </button>
      <span className="visually-hidden" aria-live="polite">
        {state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : ""}
      </span>
    </>
  );
}
