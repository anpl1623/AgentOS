import { type ReactNode, useMemo, useRef } from "react";

import { Icon } from "../../components/common";
import { lineCount } from "./policyModel";

/**
 * A text area dressed as a code editor: a bar naming the file, and a gutter of
 * line numbers that scrolls with the text.
 *
 * Still a plain `<textarea>`, so typing, selection, undo and the platform's
 * own accessibility all behave as they do in every other field.
 * Lines do not wrap, which is what keeps each number beside its line; a long
 * line scrolls sideways inside the editor rather than widening the page. The
 * gutter is decoration, hidden from assistive technology.
 */
export function CodeEditor({
  id,
  label,
  value,
  onChange,
  meta,
  rows = 16,
  autoFocus = false,
}: {
  id: string;
  /** The field's name, drawn in the bar as the file's. */
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Beside the name: the version being edited, whether it has changed. */
  meta?: ReactNode;
  rows?: number;
  /** Take focus when drawn: the editor was opened to be typed in. */
  autoFocus?: boolean;
}) {
  const gutter = useRef<HTMLDivElement>(null);
  const lines = lineCount(value);
  const numbers = useMemo(
    () => Array.from({ length: lines }, (_, index) => index + 1).join("\n"),
    [lines],
  );

  return (
    <div className="code-editor">
      <div className="code-editor-bar">
        <Icon name="shield" size={14} />
        <label htmlFor={id} className="code-editor-label">
          {label}
        </label>
        {meta}
        <span className="code-editor-lines">{lines === 1 ? "1 line" : `${lines} lines`}</span>
      </div>
      <div className="code-editor-body">
        <div className="code-gutter" ref={gutter} aria-hidden="true">
          {numbers}
        </div>
        <textarea
          id={id}
          rows={rows}
          autoFocus={autoFocus}
          wrap="off"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onScroll={(event) => {
            if (gutter.current) gutter.current.scrollTop = event.currentTarget.scrollTop;
          }}
        />
      </div>
    </div>
  );
}
