import { type ReactNode, useState } from "react";

/**
 * An on/off control that takes effect at once: a setting, not a form field
 * waiting for Save.
 *
 * A real button with `role="switch"`, so Space and Enter, focus and the
 * announcement of its state ("on", "off") come from the platform. The state is
 * read from `aria-checked` by the stylesheet, so what is drawn and what is
 * announced cannot disagree. Give it a visible `label`, or an `aria-label`
 * when the words that name it sit elsewhere on the row; `describedBy` points
 * at what pressing it will do, where its name cannot say.
 *
 * While `busy` — a change is in flight that the switch must wait for — it is
 * never disabled, since a focused control that becomes disabled drops focus
 * to the top of the window. It says it is busy, ignores presses, and is drawn
 * and announced at the state it was last pressed towards, so the knob moves
 * when it is pressed rather than when the runtime answers, and goes back if
 * the answer is no. Busy with work it was not pressed for, it holds where it
 * is.
 */
export function Switch({
  checked,
  onChange,
  label,
  ariaLabel,
  describedBy,
  busy = false,
  disabled = false,
  tip,
  className,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Drawn beside the switch, and its name. */
  label?: ReactNode;
  /** The name when there is no visible label. */
  ariaLabel?: string | undefined;
  /** The id of what describes it, such as a note on what turning it on will do. */
  describedBy?: string | undefined;
  /** A change is in flight; presses are ignored until it is done. */
  busy?: boolean | undefined;
  /** Unavailable. Never while busy: see above. */
  disabled?: boolean | undefined;
  /** A tooltip, such as what pressing it will do; never its only name. */
  tip?: string | undefined;
  /** Added after `switch`, for a screen's layout. */
  className?: string | undefined;
}) {
  // What the switch was last pressed towards, forgotten as soon as it is not
  // busy, so it only ever stands for the change still in flight. Reset during
  // render, as React allows for state derived from props.
  const [asked, setAsked] = useState<boolean | null>(null);
  if (!busy && asked !== null) setAsked(null);
  const shown = busy && asked !== null ? asked : checked;

  return (
    <button
      type="button"
      role="switch"
      className={className ? `switch ${className}` : "switch"}
      aria-checked={shown}
      aria-label={ariaLabel}
      aria-describedby={describedBy}
      aria-busy={busy || undefined}
      aria-disabled={busy || undefined}
      data-tip={tip}
      disabled={disabled && !busy}
      onClick={() => {
        if (busy) return;
        setAsked(!checked);
        onChange(!checked);
      }}
    >
      <span className="switch-track" aria-hidden="true">
        <span className="switch-knob" />
      </span>
      {label === undefined ? null : <span className="switch-label">{label}</span>}
    </button>
  );
}
