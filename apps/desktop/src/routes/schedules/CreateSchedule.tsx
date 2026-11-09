import { type ReactNode, useState } from "react";

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { ScheduleView } from "../../bindings/ScheduleView";
import { EmptyState, ErrorBanner, Icon, Loading } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { useDraft } from "../../sdk/drafts";
import { CadenceFields } from "./CadenceFields";
import { CadencePreview } from "./CadencePreview";
import { FieldError } from "./FieldError";
import { previewWords } from "./cadenceModel";
import {
  type CadenceForm,
  type ScheduleForm,
  blankForm,
  cadenceInput,
  cadenceVerdict,
  chosenAgent,
  isDirty,
  previewStops,
  scheduleInput,
} from "./formModel";
import { useCadenceCheck } from "./useCadenceCheck";
import {
  type FieldsLeft,
  cadenceFieldError,
  fieldErrors,
  nameClash,
  submitState,
} from "./validationModel";

/** Where an unfinished new schedule is kept between visits to the screen. */
const DRAFT_KEY = "schedules.new";

/**
 * The "New schedule" section: the form, or what stands in its way.
 *
 * Its heading takes focus when the page header's New schedule is pressed and
 * there is no form to start in, so the person lands on the reason.
 */
export function NewScheduleSection({
  agents,
  agentsError,
  now,
  takenNames,
  onCreated,
  openAgents,
}: {
  /** The agents that can be given work; `null` until read. */
  agents: readonly AgentSummary[] | null;
  agentsError: string | null;
  now: number;
  takenNames: readonly string[];
  onCreated: (schedule: ScheduleView) => void;
  openAgents: () => void;
}) {
  let body: ReactNode;
  if (agents === null) {
    body = agentsError ? null : (
      <div className="panel-body">
        <Loading what="agents" />
      </div>
    );
  } else if (agents.length === 0) {
    body = (
      <EmptyState
        icon="agents"
        title="No enabled agent to give a schedule to"
        action={
          <button type="button" onClick={openAgents}>
            Go to Agents
            <Icon name="chevron-right" size={14} />
          </button>
        }
      >
        <p>Each firing gives its objective to an agent, and none can be given work right now.</p>
      </EmptyState>
    );
  } else {
    body = (
      <CreateSchedule agents={agents} now={now} takenNames={takenNames} onCreated={onCreated} />
    );
  }

  return (
    <section className="schedule-section" aria-labelledby="schedules-new">
      <h2 id="schedules-new" tabIndex={-1}>
        New schedule
      </h2>
      {agentsError ? (
        <ErrorBanner message={`The agents could not be read: ${agentsError}`} />
      ) : null}
      {body === null ? null : <div className="panel schedule-form">{body}</div>}
    </section>
  );
}

/**
 * The create form: what it does on the left, when it fires on the right, and
 * Create beneath both. One column when the panel is narrow.
 *
 * Held as a draft, so a half-written objective survives a visit to another
 * screen. No leave guard: leaving loses nothing, and asking would be a
 * question with only one sensible answer.
 *
 * The name and objective say what is wrong once they have been left, not
 * while they are first being typed into; a name another schedule has is said
 * at once, since it is news rather than a nag. The cadence says what is wrong
 * as it is typed, because the runtime's answer is the point of typing it.
 */
function CreateSchedule({
  agents,
  now,
  takenNames,
  onCreated,
}: {
  agents: readonly AgentSummary[];
  now: number;
  takenNames: readonly string[];
  onCreated: (schedule: ScheduleView) => void;
}) {
  const [blank] = useState(() => blankForm(Date.now()));
  const [form, setForm, clearForm] = useDraft<ScheduleForm>(DRAFT_KEY, blank);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState<FieldsLeft>({ name: false, objective: false });

  const agentId = chosenAgent(form.agentId, agents);
  const cadence = form.cadence;
  const local = cadenceInput(cadence);
  const check = useCadenceCheck(local.ok && cadence.kind !== "once" ? local.value : null);
  const verdict = cadenceVerdict(cadence, now, check.preview, check.error);
  const ready = scheduleInput(form, agentId, now);
  const submit = submitState(ready, verdict, nameClash(form.name, takenNames));
  const { name: nameError, objective: objectiveError } = fieldErrors(form, takenNames, left);

  const set = (fields: Partial<ScheduleForm>) => setForm((previous) => ({ ...previous, ...fields }));
  const setCadence = (fields: Partial<CadenceForm>) =>
    setForm((previous) => ({ ...previous, cadence: { ...previous.cadence, ...fields } }));
  const startOver = () => {
    clearForm();
    setError(null);
    setLeft({ name: false, objective: false });
  };

  const create = async () => {
    // Read against the clock now rather than the minute-old `now`, so a
    // one-shot that has slipped into the past while the form sat is refused
    // here instead of firing on the next tick.
    const input = scheduleInput(form, agentId, Date.now());
    if (!input.ok) {
      setError(input.error);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const schedule = await api.createSchedule(input.value);
      clearForm();
      setLeft({ name: false, objective: false });
      onCreated(schedule);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="schedule-form-grid">
        <section className="schedule-form-part" aria-labelledby="schedule-what">
          <h3 className="schedule-form-title" id="schedule-what">
            <Icon name="tasks" size={16} />
            What it does
          </h3>
          <div className="field">
            <label htmlFor="schedule-agent">Agent</label>
            <select
              id="schedule-agent"
              value={agentId}
              onChange={(event) => set({ agentId: event.target.value })}
            >
              {agents.map((agent) => (
                <option key={agent.id} value={agent.id}>
                  {agent.name} · {agent.provider}/{agent.model}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="schedule-name">Name</label>
            <input
              id="schedule-name"
              value={form.name}
              placeholder="weekday-follow-ups"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={nameError !== null || undefined}
              aria-describedby={nameError !== null ? "schedule-name-error" : undefined}
              onChange={(event) => set({ name: event.target.value })}
              onBlur={() => setLeft((was) => ({ ...was, name: true }))}
            />
            {nameError !== null ? (
              <FieldError id="schedule-name-error">{nameError}</FieldError>
            ) : null}
          </div>
          <div className="field schedule-objective-field">
            <label htmlFor="schedule-objective">Objective · what each firing is given</label>
            <textarea
              id="schedule-objective"
              rows={5}
              value={form.objective}
              placeholder="Find every customer whose follow-up is overdue and draft a message for each."
              aria-invalid={objectiveError !== null || undefined}
              aria-describedby={objectiveError !== null ? "schedule-objective-error" : undefined}
              onChange={(event) => set({ objective: event.target.value })}
              onBlur={() => setLeft((was) => ({ ...was, objective: true }))}
            />
            {objectiveError !== null ? (
              <FieldError id="schedule-objective-error">{objectiveError}</FieldError>
            ) : null}
          </div>
          <div className="schedule-unwatched">
            <Icon name="shield" size={16} />
            <p>
              <strong>Runs unattended.</strong> Nobody watches a scheduled run, so every approval it
              would ask for is refused with a note the agent can read and plan around.
            </p>
          </div>
        </section>

        <section className="schedule-form-part" aria-labelledby="schedule-when">
          <h3 className="schedule-form-title" id="schedule-when">
            <Icon name="clock" size={16} />
            When it fires
          </h3>
          <CadenceFields
            cadence={cadence}
            error={cadenceFieldError(verdict)}
            onChange={setCadence}
          />
          <CadencePreview
            verdict={verdict}
            words={previewWords(cadence, verdict)}
            stops={previewStops(cadence.kind)}
          />
        </section>
      </div>

      <div className="schedule-form-foot">
        {error ? <ErrorBanner message={error} /> : null}
        <div className="schedule-submit">
          <button
            type="button"
            className="primary"
            disabled={busy || submit.disabled}
            onClick={() => void create()}
          >
            <Icon name="plus" size={16} />
            {busy ? "Creating…" : "Create schedule"}
          </button>
          {isDirty(form, blank) ? (
            <button type="button" className="ghost" disabled={busy} onClick={startOver}>
              Start over
            </button>
          ) : null}
          {submit.reason !== null ? <span className="faint">{submit.reason}</span> : null}
        </div>
      </div>
    </>
  );
}
