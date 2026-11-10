import { Fragment, type ReactNode } from "react";

import { Icon } from "../../components/common";
import { CadenceError, CADENCE_ERROR_ID } from "./FieldError";
import { CRON_PRESETS, INTERVAL_PRESETS, cadenceIcon } from "./cadenceModel";
import { CADENCE_KINDS, type CadenceForm, type CronClock } from "./formModel";
import { spanOfSeconds } from "./timeModel";

const CLOCKS = [
  ["local", "Local time"],
  ["utc", "UTC"],
] as const satisfies readonly (readonly [CronClock, string])[];

/**
 * The choice of cadence kind, then the fields of the kind chosen.
 *
 * The kinds are three pressable cards rather than a segmented control, so each
 * can say in a line what it means before it is chosen. Every kind keeps its
 * fields while another is chosen (see `CadenceForm`).
 *
 * `error` is why the cadence is refused, or `null`. It is drawn directly
 * beneath the field being typed in, which is marked invalid and described by
 * it, so the reason sits where the eye already is rather than below the
 * presets.
 */
export function CadenceFields({
  cadence,
  error,
  onChange,
}: {
  cadence: CadenceForm;
  error: string | null;
  onChange: (fields: Partial<CadenceForm>) => void;
}) {
  const invalid =
    error === null ? {} : { "aria-invalid": true, "aria-describedby": CADENCE_ERROR_ID };
  const kind = kindFields(cadence, invalid, onChange);
  return (
    <>
      <div className="field">
        <span id="schedule-cadence-label" className="field-label">
          Cadence
        </span>
        <div className="cadence-kinds" role="group" aria-labelledby="schedule-cadence-label">
          {CADENCE_KINDS.map(({ kind: each, label, note }) => (
            <button
              key={each}
              type="button"
              className="cadence-kind lift"
              data-spotlight
              aria-pressed={cadence.kind === each}
              onClick={() => onChange({ kind: each })}
            >
              <span className="cadence-kind-icon" aria-hidden="true">
                <Icon name={cadenceIcon(each)} size={16} />
              </span>
              <span className="cadence-kind-name">{label}</span>
              <span className="cadence-kind-note">{note}</span>
            </button>
          ))}
        </div>
      </div>
      {/* The same four children whichever kind is chosen, so the error's live
          region stays mounted across a change of kind and the first refusal
          after one is still heard. The input is keyed on the kind, so the
          seconds field is never reused as the expression field. */}
      <div className="field">
        <label htmlFor={kind.inputId}>{kind.label}</label>
        <Fragment key={cadence.kind}>{kind.input}</Fragment>
        <CadenceError error={error} />
        {kind.extras}
      </div>
    </>
  );
}

/** The parts of one kind's field: what it is called, what is typed, and what helps. */
interface KindParts {
  inputId: string;
  label: ReactNode;
  input: ReactNode;
  /** One-press choices and notes, beneath the error. */
  extras: ReactNode;
}

function kindFields(
  cadence: CadenceForm,
  invalid: Record<string, unknown>,
  onChange: (fields: Partial<CadenceForm>) => void,
): KindParts {
  switch (cadence.kind) {
    case "every": {
      const seconds = Number(cadence.seconds.trim());
      const span =
        /^\d+$/.test(cadence.seconds.trim()) && seconds > 0 ? spanOfSeconds(seconds) : null;
      return {
        inputId: "schedule-seconds",
        label: "Seconds between firings",
        input: (
          <div className="inline">
            <input
              id="schedule-seconds"
              type="number"
              inputMode="numeric"
              min={1}
              step={1}
              value={cadence.seconds}
              onChange={(event) => onChange({ seconds: event.target.value })}
              {...invalid}
            />
            {span && span !== `${seconds}s` ? <span className="faint">{span}</span> : null}
          </div>
        ),
        extras: (
          <div className="cadence-presets" role="group" aria-label="Common intervals">
            {INTERVAL_PRESETS.map((preset) => (
              <button
                key={preset.seconds}
                type="button"
                className="cadence-preset"
                aria-pressed={cadence.seconds.trim() === String(preset.seconds)}
                onClick={() => onChange({ seconds: String(preset.seconds) })}
              >
                {preset.words}
              </button>
            ))}
          </div>
        ),
      };
    }
    case "cron":
      return {
        inputId: "schedule-expression",
        label: "Expression · minute hour day-of-month month day-of-week",
        input: (
          <div className="inline cadence-cron">
            <input
              id="schedule-expression"
              className="mono"
              value={cadence.expression}
              placeholder="0 9 * * MON-FRI"
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => onChange({ expression: event.target.value })}
              {...invalid}
            />
            <div className="cadence-clock">
              <span id="schedule-clock-label" className="muted">
                read in
              </span>
              <div className="segmented" role="group" aria-labelledby="schedule-clock-label">
                {CLOCKS.map(([clock, label]) => (
                  <button
                    key={clock}
                    type="button"
                    aria-pressed={cadence.clock === clock}
                    onClick={() => onChange({ clock })}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        ),
        extras: (
          <>
            <div className="cadence-presets" role="group" aria-label="Common patterns">
              {CRON_PRESETS.map((preset) => (
                <button
                  key={preset.expression}
                  type="button"
                  className="cadence-preset"
                  aria-pressed={cadence.expression.trim() === preset.expression}
                  onClick={() => onChange({ expression: preset.expression })}
                >
                  {preset.words}
                </button>
              ))}
            </div>
            <p className="field-note">
              Days of the week count from Sunday as 1 here, so{" "}
              <code className="mono">MON-FRI</code> reads more surely than{" "}
              <code className="mono">1-5</code>, which means Sunday to Thursday.
            </p>
          </>
        ),
      };
    case "once":
      return {
        inputId: "schedule-at",
        label: "When · in this machine's time zone",
        input: (
          <input
            id="schedule-at"
            type="datetime-local"
            className="select-compact"
            value={cadence.at}
            onChange={(event) => onChange({ at: event.target.value })}
            {...invalid}
          />
        ),
        extras: null,
      };
  }
}
