import type { ReactNode } from "react";

import { Icon } from "../../components/common";

/** The cadence's inline error, which the field being typed in points at. */
export const CADENCE_ERROR_ID = "schedule-cadence-error";

/**
 * A field's own error, directly beneath it, named by the field's
 * `aria-describedby`. The alert mark goes with the hue, so colour is not the
 * only signal.
 */
export function FieldError({ id, children }: { id: string; children: ReactNode }) {
  return (
    <p className="field-error" id={id}>
      <Icon name="alert" size={14} />
      <span>{children}</span>
    </p>
  );
}

/**
 * The cadence's error, under whichever field the chosen kind types into.
 *
 * The region is always present, empty while the cadence is fine, so the first
 * refusal is heard as well as the ones after it; the runtime's answer arrives
 * while focus is still in the field.
 */
export function CadenceError({ error }: { error: string | null }) {
  return (
    <div aria-live="polite">
      {error === null ? null : <FieldError id={CADENCE_ERROR_ID}>{error}</FieldError>}
    </div>
  );
}
