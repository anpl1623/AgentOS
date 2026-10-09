import { Fragment, useMemo, useState } from "react";

import {
  blankWarning,
  byteSize,
  classifyArguments,
  copyableJson,
  countHidden,
  delimiters,
  drawnLines,
  escapeLabel,
  folded,
  hiddenWarning,
  kilobytes,
  segments,
  sizeNote,
} from "./argumentText";
import { useCopy } from "./hooks";

/**
 * Matches an element the keyboard scrolls, marked `data-scroll-region`. A
 * screen's own arrow-key accelerators stand aside inside one, or a keyboard
 * user could not read to its end.
 */
export const SCROLL_REGION = "[data-scroll-region]";

/**
 * The exact arguments of a tool call.
 *
 * Fields lists the top-level keys one per line, a string value in quotes so
 * it cannot pass for a literal; Raw is the JSON text exactly as sent, never
 * summarised or shortened, because the point of the block is that it shows
 * what will run. In both, a character that draws as nothing or
 * turns text around is drawn as a boxed `⟨U+…⟩` instead, and a line above the
 * block says how many there are.
 *
 * `decided` is for a request already answered. Only then are long strings
 * folded: while a decision is pending, a fold is a place to hide the part of a
 * payload that matters, so a pending block shows everything and scrolls.
 */
export function ArgumentBlock({
  json,
  id,
  decided = false,
}: {
  /** The arguments, as the runtime's JSON text. */
  json: string;
  /** Unique on the page; the block's element ids are built from it. */
  id: string;
  decided?: boolean | undefined;
}) {
  const view = useMemo(() => classifyArguments(json), [json]);
  const [mode, setMode] = useState<"fields" | "raw">("fields");
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(() => new Set());
  const { copy, state } = useCopy();

  const shown = view.kind === "fields" ? mode : "raw";
  const hidden = useMemo(
    () =>
      shown === "raw" || view.kind !== "fields"
        ? countHidden(json)
        : view.fields.reduce(
            (sum, field) => sum + countHidden(field.key) + countHidden(field.text),
            0,
          ),
    [json, shown, view],
  );
  const warning = hiddenWarning(hidden);
  // Measured on what the view draws: in Fields a string's escaped newlines
  // are line breaks, so one line of JSON can be a screenful of blank lines.
  const size =
    shown === "fields" && view.kind === "fields"
      ? sizeNote(json, drawnLines(view.fields))
      : sizeNote(json);
  const blank = blankWarning(
    view.kind === "fields" ? view.fields.map((field) => field.text) : [json],
  );
  const labelId = `${id}-label`;

  return (
    <div className="field">
      <div className="inline">
        <label id={labelId}>Exact arguments</label>
        {view.kind === "fields" ? (
          <div className="segmented" role="group" aria-label="Show the arguments as">
            <button
              type="button"
              aria-pressed={mode === "fields"}
              onClick={() => setMode("fields")}
            >
              Fields
            </button>
            <button type="button" aria-pressed={mode === "raw"} onClick={() => setMode("raw")}>
              Raw
            </button>
          </div>
        ) : null}
        <span className="spacer" />
        <button type="button" className="ghost" onClick={() => void copy(copyableJson(json))}>
          {state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : "Copy JSON"}
        </button>
        <span className="visually-hidden" role="status">
          {state === "copied"
            ? "Arguments copied, with invisible characters written as escapes."
            : state === "failed"
              ? "The arguments could not be copied."
              : ""}
        </span>
      </div>

      {view.kind === "raw" ? <p className="field-note">{view.why}</p> : null}
      {warning !== null ? (
        <div className="banner warn" role="note">
          {warning}
        </div>
      ) : null}
      {blank !== null ? (
        <div className="banner warn" role="note">
          {blank}
        </div>
      ) : null}
      {size !== null ? <p className="faint">{size}</p> : null}

      {shown === "raw" || view.kind !== "fields" ? (
        // Focusable so a keyboard can scroll it: without the tab stop, the
        // rest of a long payload is out of reach of anyone not using a mouse.
        <pre
          className="code wrap"
          tabIndex={0}
          role="group"
          aria-labelledby={labelId}
          data-scroll-region=""
        >
          <Visible text={json} />
        </pre>
      ) : (
        <dl className="args" aria-labelledby={labelId}>
          {view.fields.map((field, index) => {
            const fold =
              decided && field.string && !expanded.has(index) ? folded(field.text) : null;
            const [open, close] = delimiters(field);
            return (
              <Fragment key={index}>
                <dt>
                  <Visible text={field.key} />
                </dt>
                <dd>
                  <span className="arg-value">
                    <span className="faint">{open}</span>
                    <Visible text={fold ?? field.text} />
                    {/* A folded string is left open: its end is not shown. */}
                    {fold !== null ? "…" : <span className="faint">{close}</span>}
                  </span>
                  {fold !== null ? (
                    <div>
                      <button
                        type="button"
                        className="arg-fold"
                        onClick={() => setExpanded((open) => new Set(open).add(index))}
                      >
                        show all ({kilobytes(byteSize(field.text))})
                      </button>
                    </div>
                  ) : null}
                </dd>
              </Fragment>
            );
          })}
        </dl>
      )}
    </div>
  );
}

/**
 * Text with every hidden code point drawn as a boxed escape.
 *
 * Boxed rather than written inline, so an argument that spells out `⟨U+202E⟩`
 * as ordinary characters cannot pass for an escape the block drew. Used for
 * anything on an approval card built from what the model sent, not only the
 * arguments: a summary or a resource name carries the same characters.
 */
export function Visible({ text }: { text: string }) {
  return (
    <>
      {segments(text).map((segment, index) =>
        segment.kind === "text" ? (
          <Fragment key={index}>{segment.text}</Fragment>
        ) : (
          <span key={index} className="tag" title="An invisible or direction-changing character">
            {escapeLabel(segment.codePoint)}
          </span>
        ),
      )}
    </>
  );
}
