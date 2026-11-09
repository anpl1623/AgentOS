import { type CSSProperties, useEffect, useId, useRef } from "react";

import type { MemoryView } from "../../bindings/MemoryView";
import { countHidden, hiddenWarning, visibleText } from "../../components/argumentText";
import { Icon, Row, Tainted } from "../../components/common";
import { ago, humanise } from "../../sdk/format";
import { confidenceBand, confidenceLabel, confidencePercent, shortName } from "./memoryModel";

/** A memory being revised in place. */
export interface MemoryEdit {
  id: string;
  content: string;
  confidence: number;
}

/** The confidence slider's step. */
const CONFIDENCE_STEP = 0.05;

/**
 * One remembered item, read.
 *
 * Content and source are drawn through `visibleText`: a memory can be recorded
 * from a page the agent read, and an invisible or direction-changing character
 * in it would otherwise reach the operator as the character itself.
 */
export function MemoryLine({
  memory,
  busy,
  refocus = false,
  onEdit,
  onForget,
}: {
  memory: MemoryView;
  busy: boolean;
  /** Take focus on the Edit button: this row's editor has just closed. */
  refocus?: boolean | undefined;
  onEdit: () => void;
  onForget: () => void;
}) {
  const edit = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (refocus) edit.current?.focus();
  }, [refocus]);
  const warning = hiddenWarning(countHidden(memory.content) + countHidden(memory.source));
  // Every row has an Edit and a Forget; a list of buttons read aloud needs to
  // say which memory each one is for.
  const named = shortName(visibleText(memory.content));
  return (
    <Row className={memory.reaches_the_prompt ? "memory" : "memory dimmed"}>
      <div className="row-main">
        <div className="row-title prose">{visibleText(memory.content)}</div>
        <div className="row-meta">
          <span className="mono">{visibleText(memory.source)}</span>
          <span>{ago(memory.updated_at)}</span>
          <ConfidenceMeter confidence={memory.confidence} />
          {memory.source_untrusted ? (
            <Tainted
              label="recorded from an outside source"
              explanation="This memory was recorded from data outside the trust boundary."
            />
          ) : null}
        </div>
        {memory.reaches_the_prompt ? null : (
          <div className="row-meta">
            <span>
              Kept, but not retrieved before planning: the model is not shown it up front.
            </span>
          </div>
        )}
        {warning ? (
          <div className="row-meta">
            <span>{warning}</span>
          </div>
        ) : null}
      </div>
      <div className="memory-actions">
        <span className="verdict neutral">
          <span className="visually-hidden">kind </span>
          {humanise(memory.kind)}
        </span>
        <button ref={edit} type="button" className="ghost" disabled={busy} onClick={onEdit}>
          <Icon name="edit" size={15} />
          Edit<span className="visually-hidden"> {named}</span>
        </button>
        <button type="button" className="ghost" disabled={busy} onClick={onForget}>
          <Icon name="trash" size={15} />
          Forget<span className="visually-hidden"> {named}</span>
        </button>
      </div>
    </Row>
  );
}

/**
 * A confidence as a short bar and the number beside it. The bar is drawn in
 * the band's hue and hidden from assistive technology; the words carry it.
 */
function ConfidenceMeter({ confidence }: { confidence: number }) {
  return (
    <span className="confidence">
      <span
        className={`confidence-track band-${confidenceBand(confidence)}`}
        style={{ "--w": `${confidencePercent(confidence)}%` } as CSSProperties}
        aria-hidden="true"
      >
        <span className="confidence-fill" />
      </span>
      {confidenceLabel(confidence)}
    </span>
  );
}

/** One remembered item, being revised. */
export function MemoryEditor({
  memory,
  draft,
  busy,
  onChange,
  onCancel,
  onSave,
}: {
  memory: MemoryView;
  draft: MemoryEdit;
  busy: boolean;
  onChange: (next: MemoryEdit) => void;
  onCancel: () => void;
  onSave: (content: string, confidence: number) => void;
}) {
  const contentId = useId();
  const confidenceId = useId();
  return (
    <Row className="stacked memory-editing">
      <div className="field">
        <label htmlFor={contentId}>Revise this {humanise(memory.kind)}</label>
        <textarea
          id={contentId}
          rows={3}
          autoFocus
          value={draft.content}
          onChange={(event) => onChange({ ...draft, content: event.target.value })}
        />
      </div>
      <ConfidenceField
        id={confidenceId}
        value={draft.confidence}
        onChange={(confidence) => onChange({ ...draft, confidence })}
      />
      <div className="inline">
        <button
          type="button"
          className="primary"
          disabled={busy || draft.content.trim() === ""}
          onClick={() => onSave(draft.content, draft.confidence)}
        >
          {busy ? "Saving…" : "Save"}
        </button>
        <button type="button" className="ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </Row>
  );
}

/** A confidence, set with a slider, with its value said beside it. */
export function ConfidenceField({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number;
  onChange: (value: number) => void;
}) {
  const label = confidenceLabel(value);
  return (
    <div className="field confidence-field">
      <label htmlFor={id}>Confidence</label>
      <div className="inline">
        <input
          id={id}
          type="range"
          min={0}
          max={1}
          step={CONFIDENCE_STEP}
          value={value}
          aria-valuetext={label}
          onChange={(event) => onChange(Number(event.target.value))}
        />
        <output htmlFor={id} className="muted">
          {label}
        </output>
      </div>
    </div>
  );
}
