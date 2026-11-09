import { useId, useState } from "react";

import { ErrorBanner, Icon, SkeletonRows, Switch } from "../../components/common";
import { useNow } from "../../components/hooks";
import { api, describeError } from "../../sdk/client";
import { useUnsavedGuard } from "../../sdk/drafts";
import type { SchedulerState } from "./hooks";
import { Fact, Section } from "./parts";
import {
  readPacing,
  type SchedulerFact,
  schedulerFacts,
  schedulerNotices,
  schedulerSummary,
} from "./schedulerModel";

/**
 * The switch for unattended work on this machine, and what it would find.
 *
 * The switch takes effect as it is pressed, which is what a switch promises.
 * Turning it off never waits on the pacing fields: a stop is always possible,
 * whatever has been typed beside it. The pacing fields follow the scheduler
 * until someone types in them, and follow it again once what was typed has
 * been applied.
 */
export function Scheduler({ scheduler }: { scheduler: SchedulerState }) {
  const now = useNow();
  const fieldId = useId();
  const { status, view } = scheduler;

  const [tickText, setTickText] = useState<string | null>(null);
  const [maxText, setMaxText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);

  const pacing = view
    ? readPacing(
        view,
        tickText ?? String(view.tick_seconds),
        maxText ?? String(view.max_concurrent_runs),
      )
    : null;
  useUnsavedGuard(
    pacing?.changed === true,
    "The scheduler's pacing has been changed and not applied.",
  );

  const apply = async (enabled: boolean) => {
    if (!view || !pacing) return;
    setBusy(true);
    setRefusal(null);
    try {
      const tick = (pacing.valid ? pacing.tick : null) ?? view.tick_seconds;
      const max = (pacing.valid ? pacing.max : null) ?? view.max_concurrent_runs;
      scheduler.answered(await api.setSchedulerRunning(enabled, tick, max));
      setTickText(null);
      setMaxText(null);
    } catch (failure) {
      setRefusal(describeError(failure));
    } finally {
      setBusy(false);
      status.reload();
    }
  };

  const notices = schedulerNotices(view, refusal);

  return (
    <Section
      id="settings-scheduler"
      intro="Unattended work on this machine: schedules that fire, and queued tasks that start when what they wait for is done."
    >
      {status.error ? (
        <ErrorBanner
          message={
            view
              ? `The scheduler's state could not be refreshed: ${status.error}`
              : `The scheduler's state could not be read: ${status.error}`
          }
        />
      ) : null}
      {notices.map((notice) => (
        <div key={notice} className="banner warn" role="status">
          {notice}
        </div>
      ))}
      <div className="panel">
        {view === null || pacing === null ? (
          status.error ? null : (
            <SkeletonRows count={2} />
          )
        ) : (
          <>
            <div className="settings-scheduler-head">
              {/* Not disabled while a change is in flight: a focused control
                  that becomes disabled drops focus to the top of the window.
                  A press then is ignored instead, and "Working…" says why. */}
              <Switch
                checked={view.running}
                disabled={!view.running && !pacing.valid}
                onChange={(running) => {
                  if (!busy) void apply(running);
                }}
                label="Scheduler"
              />
              {view.running ? (
                <Fact tone="live" noun="scheduler">
                  Running
                </Fact>
              ) : (
                <Fact tone="neutral" noun="scheduler">
                  Off
                </Fact>
              )}
              <span className="faint" role="status">
                {busy ? "Working…" : ""}
              </span>
              <p className="settings-scheduler-summary">{schedulerSummary(view, now)}</p>
            </div>

            <dl className="settings-facts">
              {schedulerFacts(view, now).map((fact) => (
                <FactTile key={fact.label} fact={fact} />
              ))}
            </dl>

            <div className="settings-scheduler-pacing">
              <div className="settings-form-grid">
                <div className="field">
                  <label htmlFor={`${fieldId}-tick`}>Tick, in seconds</label>
                  <input
                    id={`${fieldId}-tick`}
                    type="number"
                    min={5}
                    step={1}
                    inputMode="numeric"
                    value={tickText ?? String(view.tick_seconds)}
                    onChange={(event) => setTickText(event.target.value)}
                  />
                </div>
                <div className="field">
                  <label htmlFor={`${fieldId}-max`}>Runs at once, at most</label>
                  <input
                    id={`${fieldId}-max`}
                    type="number"
                    min={1}
                    step={1}
                    inputMode="numeric"
                    value={maxText ?? String(view.max_concurrent_runs)}
                    onChange={(event) => setMaxText(event.target.value)}
                  />
                </div>
              </div>
              {(view.running && pacing.changed) || !pacing.valid ? (
                <div className="settings-form-actions settings-enter">
                  {view.running && pacing.changed ? (
                    <button
                      type="button"
                      className="primary"
                      disabled={busy || !pacing.valid}
                      onClick={() => void apply(true)}
                    >
                      Apply pacing
                    </button>
                  ) : null}
                  {!pacing.valid ? (
                    <span className="field-note settings-invalid">
                      Both fields need a whole number.
                    </span>
                  ) : null}
                </div>
              ) : null}
            </div>

            <ul className="settings-notes">
              <li>
                <Icon name="lock" size={14} />
                <span>
                  A scheduled run has nobody watching it, so anything the policy would have
                  escalated to a person is refused, with a note the agent can re-plan around.
                </span>
              </li>
              <li>
                <Icon name="window" size={14} />
                <span>
                  Schedules fire only while this application is open, or while{" "}
                  <span className="mono">agentos schedule run</span> runs in a terminal. Turning
                  the scheduler on is remembered, and it starts again at the next launch.
                </span>
              </li>
            </ul>
          </>
        )}
      </div>
    </Section>
  );
}

/**
 * One of the scheduler's counts. A count that asks for a look keeps the size
 * of its neighbours, so it is not the smallest figure in the row, and is told
 * apart by its hue, its edge and the alert mark, with words for a screen
 * reader that the mark stands in for.
 */
function FactTile({ fact }: { fact: SchedulerFact }) {
  return (
    <div className={fact.attention ? "settings-fact attention" : "settings-fact"}>
      <dt>{fact.label}</dt>
      <dd>
        {fact.attention ? (
          <span className="settings-fact-flag">
            <Icon name="alert" size={16} />
            {fact.value}
            <span className="visually-hidden">, which needs a look</span>
          </span>
        ) : (
          fact.value
        )}
      </dd>
    </div>
  );
}
