import type { ReactNode } from "react";

/**
 * An on/off control that takes effect at once: a setting, not a form field
 * waiting for Save.
 *
 * A real button with `role="switch"`, so Space and Enter, focus and the
 * announcement of its state ("on", "off") come from the platform. The state is
 * read from `aria-checked` by the stylesheet, so what is drawn and what is
 * announced cannot disagree. Give it a visible `label`, or an `aria-label`
 * when the words that name it sit elsewhere on the row.
 */
export function Switch({
  checked,
  onChange,
  label,
  ariaLabel,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Drawn beside the switch, and its name. */
  label?: ReactNode;
  /** The name when there is no visible label. */
  ariaLabel?: string | undefined;
  disabled?: boolean | undefined;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-track" aria-hidden="true">
        <span className="switch-knob" />
      </span>
      {label === undefined ? null : <span className="switch-label">{label}</span>}
    </button>
  );
}
