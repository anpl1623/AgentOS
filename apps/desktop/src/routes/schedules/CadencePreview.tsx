import type { ReactNode } from "react";

import { Icon, LiveDot } from "../../components/common";
import type { CadenceVerdict } from "./formModel";

/**
 * Beneath the cadence, as it is typed: the cadence in words over a short
 * timeline of its next firings.
 *
 * A polite live region, so the answer is heard while focus stays in the field
 * being typed in. A refusal is not said here: it is the field's own inline
 * error (`CadenceError`), directly beneath the field. While there is no answer
 * to show, the card keeps its shape with a line saying why and a timeline of
 * empty stops, `stops` long, so the form beneath does not jump as each
 * keystroke is checked; those stops shimmer only while a check is out.
 */
export function CadencePreview({
  verdict,
  words,
  stops,
}: {
  verdict: CadenceVerdict;
  words: { words: string; exact: string | null } | null;
  stops: number;
}) {
  return (
    <div className="cadence-preview" aria-live="polite">
      <PreviewBody verdict={verdict} words={words} stops={stops} />
    </div>
  );
}

function PreviewBody({
  verdict,
  words,
  stops,
}: {
  verdict: CadenceVerdict;
  words: { words: string; exact: string | null } | null;
  stops: number;
}) {
  switch (verdict.state) {
    case "invalid":
      // Hidden from assistive technology: the refusal beneath the field has
      // already been heard, and this only keeps the card's place.
      return (
        <Waiting stops={stops} hidden>
          <Icon name="calendar" size={16} />
          <span>Its next firings appear here once the cadence is fixed.</span>
        </Waiting>
      );
    case "checking":
      return (
        <Waiting stops={stops} shimmer>
          <LiveDot tone="info" />
          <span>Checking the cadence…</span>
        </Waiting>
      );
    case "unchecked":
      return (
        <Waiting stops={stops}>
          <Icon name="info" size={16} />
          <span>
            It could not be checked here ({verdict.error}). The runtime checks it again when the
            schedule is created.
          </span>
        </Waiting>
      );
    case "valid":
      return (
        <div className="cadence-preview-card">
          <div className="cadence-preview-head">
            <p className="cadence-preview-words">{words?.words ?? verdict.description}</p>
            {words?.exact ? <p className="cadence-preview-exact">{words.exact}</p> : null}
          </div>
          <p className="cadence-preview-heading">{verdict.heading}</p>
          <ol className="cadence-firings stagger" aria-label="Next firings">
            {/* Keyed by place, not by label: a cron with a seconds field can
                fire several times in one minute, and the labels are to the
                minute, so two firings can read the same. */}
            {verdict.firings.map((firing, index) => (
              <li key={index}>
                <span className="cadence-firing-dot" aria-hidden="true" />
                <span className="mono">{firing}</span>
              </li>
            ))}
          </ol>
        </div>
      );
  }
}

/** The card with no firings to show: why, over a timeline of empty stops. */
function Waiting({
  stops,
  shimmer = false,
  hidden = false,
  children,
}: {
  stops: number;
  shimmer?: boolean;
  hidden?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="cadence-preview-card is-waiting" aria-hidden={hidden || undefined}>
      <div className="cadence-preview-head">
        <p className="cadence-preview-status">{children}</p>
      </div>
      <p className="cadence-preview-heading" aria-hidden="true">
        Next firings
      </p>
      <ol className="cadence-firings is-ghost" aria-hidden="true">
        {Array.from({ length: stops }, (_, index) => (
          <li key={index}>
            <span className="cadence-firing-dot" />
            <span className={shimmer ? "cadence-ghost skeleton-line" : "cadence-ghost"} />
          </li>
        ))}
      </ol>
    </div>
  );
}
