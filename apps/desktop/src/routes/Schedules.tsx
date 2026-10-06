import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import type { AgentSummary } from "../bindings/AgentSummary";
import type { CadenceInput } from "../bindings/CadenceInput";
import type { CadencePreview } from "../bindings/CadencePreview";
import type { ScheduleView } from "../bindings/ScheduleView";
import type { SchedulerView } from "../bindings/SchedulerView";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  Empty,
  ErrorBanner,
  Loading,
  PageHeader,
  Row,
  SkeletonRows,
  Stale,
} from "../components/common";
import { useNow } from "../components/hooks";
import { api, describeError } from "../sdk/client";
import { useDraft } from "../sdk/drafts";
import { useVisibleInterval } from "../sdk/live";
import { useAsync } from "../sdk/useAsync";
import { useNavigate } from "../shell/router";
import {
  CADENCE_KINDS,
  type CadenceForm,
  type CronClock,
  type ScheduleForm,
  blankForm,
  cadenceInput,
  cadenceLabel,
  cadenceVerdict,
  chosenAgent,
  deleteMessage,
  isDirty,
  relative,
  scheduleInput,
  scheduleRow,
  sortSchedules,
  spanOfSeconds,
} from "./schedulesModel";

/**
 * How often the list and the scheduler's state are re-read while visible.
 *
 * Schedules change on their own only when one fires, and the soonest change
 * a person would notice is a minute-resolution "next in 4m"; a few seconds of
 * lag says nothing wrong.
 */
const LIST_MS = 15_000;

/**
 * How long typing must pause before the cadence is checked.
 *
 * Long enough that `0 9 * * 1-5` typed at speed is one request rather than
 * eleven, short enough that the answer arrives while the operator is still
 * looking at the field.
 */
const CHECK_DEBOUNCE_MS = 250;

/** Where an unfinished new schedule is kept between visits to the screen. */
const DRAFT_KEY = "schedules.new";

/**
 * Standing instructions: an objective given to an agent on an interval, on a
 * cron, or once.
 *
 * The scheduler's state is read beside the list on every refresh, because a
 * list of active schedules with the scheduler off is a list of things that
 * are not happening, and the screen has to say so where the list is.
 *
 * Navigation is taken from the router rather than a prop: the shell renders
 * this screen without one, and the screen is not to change the shell.
 */
export function Schedules() {
  const navigate = useNavigate();
  const now = useNow(30_000);

  const loadedAt = useRef<number | null>(null);
  const schedules = useAsync(() => api.listSchedules(), [], {
    onSuccess: () => {
      loadedAt.current = Date.now();
    },
  });
  const scheduler = useAsync(() => api.schedulerStatus(), []);
  const agents = useAsync(() => api.listAgents(), []);
  useVisibleInterval(() => {
    schedules.reload();
    scheduler.reload();
  }, LIST_MS);

  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [confirming, setConfirming] = useState<ScheduleView | null>(null);

  const reloadSchedules = schedules.reload;
  const reloadScheduler = scheduler.reload;
  const refresh = useCallback(() => {
    reloadSchedules();
    reloadScheduler();
  }, [reloadSchedules, reloadScheduler]);

  const setPaused = useCallback(
    async (schedule: ScheduleView, paused: boolean) => {
      setBusy(schedule.id);
      setActionError(null);
      try {
        const changed = await api.setSchedulePaused(schedule.id, paused);
        setAnnouncement(
          paused
            ? `Paused ${changed.name}.`
            : changed.status === "finished"
              ? `${changed.name} had nothing left to fire, so it is finished.`
              : `Resumed ${changed.name}.`,
        );
        refresh();
      } catch (failure) {
        setActionError(
          `${schedule.name} could not be ${paused ? "paused" : "resumed"}: ${describeError(failure)}`,
        );
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  const remove = useCallback(
    async (schedule: ScheduleView) => {
      setBusy(schedule.id);
      setActionError(null);
      try {
        await api.deleteSchedule(schedule.id);
        setAnnouncement(`Deleted ${schedule.name}. The tasks it created are kept.`);
        refresh();
      } catch (failure) {
        setActionError(`${schedule.name} could not be deleted: ${describeError(failure)}`);
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  const running = scheduler.data?.running;
  const created = useCallback(
    (schedule: ScheduleView) => {
      let said = `Created ${schedule.name}.`;
      if (schedule.next_run_at) {
        said += ` It first fires ${relative(schedule.next_run_at, Date.now())}`;
        said += running === false ? ", if the scheduler is on by then." : ".";
      }
      setAnnouncement(said);
      refresh();
    },
    [refresh, running],
  );

  const list = schedules.data ? sortSchedules(schedules.data) : null;
  const enabled = agents.data?.filter((agent) => agent.status === "enabled") ?? null;

  return (
    <>
      <PageHeader
        title="Schedules"
        subtitle="Standing instructions: an objective an agent is given on an interval, on a cron, or once."
      />

      <SchedulerState
        status={scheduler.data}
        error={scheduler.error}
        openSettings={() => navigate({ name: "settings" })}
      />

      <p className="visually-hidden" role="status">
        {announcement}
      </p>

      <h2 id="schedules-list">Standing</h2>
      {schedules.error ? (
        <ErrorBanner
          message={
            schedules.data
              ? `The schedules could not be re-read: ${schedules.error}`
              : `The schedules could not be read: ${schedules.error}`
          }
        />
      ) : null}
      {actionError ? <ErrorBanner message={actionError} /> : null}
      {schedules.stale && loadedAt.current !== null ? (
        <p className="muted">
          <Stale since={loadedAt.current} />
        </p>
      ) : null}
      <div className="panel">
        {schedules.loading ? (
          <>
            <Loading what="schedules" />
            <SkeletonRows count={2} />
          </>
        ) : null}
        {list?.length === 0 ? (
          <Empty>
            No schedules yet. A schedule fires only while AgentOS is open with its scheduler on, or
            while <code className="mono">agentos schedule run</code> runs.
          </Empty>
        ) : null}
        {list && list.length > 0 ? (
          <div role="list" aria-labelledby="schedules-list">
            {list.map((schedule) => (
              <div role="listitem" key={schedule.id}>
                <ScheduleRow
                  schedule={schedule}
                  now={now}
                  busy={busy === schedule.id}
                  locked={busy !== null}
                  onPause={(paused) => void setPaused(schedule, paused)}
                  onDelete={() => setConfirming(schedule)}
                  onLastTask={() => navigate({ name: "tasks" })}
                />
              </div>
            ))}
          </div>
        ) : null}
      </div>

      <h2 id="schedules-new">New schedule</h2>
      {agents.error ? (
        <ErrorBanner message={`The agents could not be read: ${agents.error}`} />
      ) : null}
      <div className="panel">
        <div className="panel-body">
          {enabled === null ? (
            agents.error ? null : (
              <Loading what="agents" />
            )
          ) : enabled.length === 0 ? (
            <p className="muted">
              No enabled agent to give a schedule to.{" "}
              <button type="button" className="ghost" onClick={() => navigate({ name: "agents" })}>
                Agents →
              </button>
            </p>
          ) : (
            <CreateSchedule agents={enabled} now={now} onCreated={created} />
          )}
        </div>
      </div>

      {confirming ? (
        <ConfirmDialog
          title={`Delete ${confirming.name}?`}
          message={deleteMessage(confirming)}
          confirmLabel="Delete schedule"
          cancelLabel="Keep it"
          onAnswer={(confirmed) => {
            const schedule = confirming;
            setConfirming(null);
            if (confirmed) void remove(schedule);
          }}
        />
      ) : null}
    </>
  );
}

/**
 * Whether anything on this screen is firing.
 *
 * The scheduler in this window is the only one the screen can see. Another,
 * started with `agentos schedule run`, would fire these schedules with this
 * window's switch off, so the warning says "unless" rather than claiming
 * nothing fires anywhere. A status that could not be read is said to be
 * unknown, not assumed in either direction.
 */
function SchedulerState({
  status,
  error,
  openSettings,
}: {
  status: SchedulerView | null;
  error: string | null;
  openSettings: () => void;
}) {
  if (status === null) {
    return error ? (
      <ErrorBanner
        message={`Whether the scheduler is running could not be read, so this screen cannot say whether these schedules fire: ${error}`}
      />
    ) : null;
  }

  if (status.running) {
    return (
      <p className="muted">
        The scheduler is running, checking every {spanOfSeconds(status.tick_seconds)}. Schedules
        fire only while AgentOS is open, or while{" "}
        <code className="mono">agentos schedule run</code> runs.
      </p>
    );
  }

  const overdue =
    status.overdue === 0
      ? null
      : status.overdue === 1
        ? "One schedule is overdue; it fires once when the scheduler starts, not once for each occurrence it missed."
        : `${status.overdue} schedules are overdue; each fires once when the scheduler starts, not once for each occurrence it missed.`;

  return (
    <div className="banner warn">
      <p>
        The scheduler is off. These schedules are kept, and fire only while AgentOS is open with
        its scheduler on, or while <code className="mono">agentos schedule run</code> runs
        elsewhere on this machine.
      </p>
      {status.error ? <p>It stopped on its own: {status.error}</p> : null}
      {overdue ? <p>{overdue}</p> : null}
      <button type="button" onClick={openSettings}>
        Turn it on in Settings →
      </button>
    </div>
  );
}

/**
 * One schedule.
 *
 * Not an activatable row: it holds its own buttons, and a button cannot sit
 * inside another. The last task is reached by its own button instead.
 */
function ScheduleRow({
  schedule,
  now,
  busy,
  locked,
  onPause,
  onDelete,
  onLastTask,
}: {
  schedule: ScheduleView;
  now: number;
  busy: boolean;
  /** Another row's change is in flight; one change at a time keeps the announcements in order. */
  locked: boolean;
  onPause: (paused: boolean) => void;
  onDelete: () => void;
  onLastTask: () => void;
}) {
  const row = scheduleRow(schedule, now);
  const noteId = `resume-note-${schedule.id}`;
  return (
    <Row className={schedule.status === "finished" ? "dimmed" : undefined}>
      <div className="row-main">
        <div className="row-title" title={`${schedule.name}: ${schedule.objective}`}>
          <span className="mono">{schedule.name}</span> · {schedule.objective}
        </div>
        <div className="row-meta">
          <span>{schedule.agent_name}</span>
          <span className="mono">{cadenceLabel(schedule.cadence)}</span>
          <span title={row.nextAt ? new Date(row.nextAt).toLocaleString() : undefined}>
            {row.next}
          </span>
          <span
            title={
              schedule.last_run_at ? new Date(schedule.last_run_at).toLocaleString() : undefined
            }
          >
            {row.last}
          </span>
        </div>
        {row.resumeNote ? (
          <div className="row-meta">
            <span className="faint" id={noteId}>
              {row.resumeNote}
            </span>
          </div>
        ) : null}
      </div>
      <span className={`verdict ${row.tone}`}>
        <span className="visually-hidden">schedule </span>
        {row.label}
      </span>
      {schedule.last_task_id ? (
        <button type="button" className="ghost" onClick={onLastTask}>
          Last task<span className="visually-hidden"> of {schedule.name}</span> →
        </button>
      ) : null}
      {row.action ? (
        <button
          type="button"
          disabled={locked}
          aria-describedby={row.action === "resume" ? noteId : undefined}
          onClick={() => onPause(row.action === "pause")}
        >
          {row.action === "pause" ? (busy ? "Pausing…" : "Pause") : busy ? "Resuming…" : "Resume"}
          <span className="visually-hidden"> {schedule.name}</span>
        </button>
      ) : null}
      <button type="button" className="danger" disabled={locked} onClick={onDelete}>
        Delete<span className="visually-hidden"> {schedule.name}</span>
      </button>
    </Row>
  );
}

/**
 * The runtime's check of the cadence as typed, settled after a pause in
 * typing.
 *
 * `preview` and `error` are only ever the answer for exactly `input`: while a
 * newer keystroke is waiting or in flight both are `null`, so an answer for
 * `0 9 * *` is never shown beneath `0 9 * * 1-5`.
 */
function useCadenceCheck(input: CadenceInput | null): {
  preview: CadencePreview | null;
  error: string | null;
} {
  const key = input === null ? null : JSON.stringify(input);
  const [settled, setSettled] = useState(key);
  useEffect(() => {
    if (key === settled) return;
    const timer = window.setTimeout(() => setSettled(key), CHECK_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [key, settled]);

  const check = useAsync<CadencePreview | null>(
    () =>
      settled === null
        ? Promise.resolve(null)
        : api.checkCadence(JSON.parse(settled) as CadenceInput),
    [settled],
  );
  const current = key === settled && !check.loading && !check.refreshing;
  return { preview: current ? check.data : null, error: current ? check.error : null };
}

/**
 * The create form.
 *
 * Held as a draft, so a half-written objective survives a visit to another
 * screen. No leave guard: leaving loses nothing, and asking would be a
 * question with only one sensible answer.
 */
function CreateSchedule({
  agents,
  now,
  onCreated,
}: {
  agents: readonly AgentSummary[];
  now: number;
  onCreated: (schedule: ScheduleView) => void;
}) {
  const [blank] = useState(() => blankForm(Date.now()));
  const [form, setForm, clearForm] = useDraft<ScheduleForm>(DRAFT_KEY, blank);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const agentId = chosenAgent(form.agentId, agents);
  const cadence = form.cadence;
  const local = cadenceInput(cadence);
  const check = useCadenceCheck(local.ok && cadence.kind !== "once" ? local.value : null);
  const verdict = cadenceVerdict(cadence, now, check.preview, check.error);
  const ready = scheduleInput(form, agentId, now);

  const set = (fields: Partial<ScheduleForm>) => setForm((previous) => ({ ...previous, ...fields }));
  const setCadence = (fields: Partial<CadenceForm>) =>
    setForm((previous) => ({ ...previous, cadence: { ...previous.cadence, ...fields } }));

  const submit = async () => {
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
      onCreated(schedule);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="grid two">
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
            onChange={(event) => set({ name: event.target.value })}
          />
        </div>
      </div>

      <div className="field">
        <label htmlFor="schedule-objective">Objective · what each firing is given</label>
        <textarea
          id="schedule-objective"
          rows={2}
          value={form.objective}
          placeholder="Find every customer whose follow-up is overdue and draft a message for each."
          onChange={(event) => set({ objective: event.target.value })}
        />
      </div>

      <div className="field">
        <div className="inline">
          <span id="schedule-cadence-label" className="muted">
            Cadence
          </span>
          <div className="segmented" role="group" aria-labelledby="schedule-cadence-label">
            {CADENCE_KINDS.map(({ kind, label }) => (
              <button
                key={kind}
                type="button"
                aria-pressed={cadence.kind === kind}
                onClick={() => setCadence({ kind })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <CadenceFields cadence={cadence} onChange={setCadence} />

      <div className="field" aria-live="polite">
        <CadencePreviewNote verdict={verdict} />
      </div>

      {error ? <ErrorBanner message={error} /> : null}

      <div className="inline">
        <button
          type="button"
          className="primary"
          disabled={busy || !ready.ok || verdict.state === "invalid"}
          onClick={() => void submit()}
        >
          {busy ? "Creating…" : "Create schedule"}
        </button>
        {isDirty(form, blank) ? (
          <button
            type="button"
            className="ghost"
            disabled={busy}
            onClick={() => {
              clearForm();
              setError(null);
            }}
          >
            Start over
          </button>
        ) : null}
        {!ready.ok && verdict.state !== "invalid" ? (
          <span className="faint">{ready.error}</span>
        ) : null}
      </div>
      <p className="field-note">
        Nobody watches a scheduled run, so every approval it would ask for is refused with a note
        the agent can read and plan around.
      </p>
    </>
  );
}

/** The fields of the chosen cadence kind. */
function CadenceFields({
  cadence,
  onChange,
}: {
  cadence: CadenceForm;
  onChange: (fields: Partial<CadenceForm>) => void;
}) {
  switch (cadence.kind) {
    case "every": {
      const seconds = Number(cadence.seconds.trim());
      const span =
        /^\d+$/.test(cadence.seconds.trim()) && seconds > 0 ? spanOfSeconds(seconds) : null;
      return (
        <div className="field">
          <label htmlFor="schedule-seconds">Seconds between firings</label>
          <div className="inline">
            <input
              id="schedule-seconds"
              type="number"
              inputMode="numeric"
              min={1}
              step={1}
              value={cadence.seconds}
              onChange={(event) => onChange({ seconds: event.target.value })}
            />
            {span && span !== `${seconds}s` ? <span className="faint">{span}</span> : null}
          </div>
        </div>
      );
    }
    case "cron":
      return (
        <div className="field">
          <label htmlFor="schedule-expression">
            Expression · minute hour day-of-month month day-of-week
          </label>
          <div className="inline">
            <input
              id="schedule-expression"
              className="mono select-compact"
              value={cadence.expression}
              placeholder="0 9 * * 1-5"
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => onChange({ expression: event.target.value })}
            />
            <span id="schedule-clock-label" className="muted">
              read in
            </span>
            <div className="segmented" role="group" aria-labelledby="schedule-clock-label">
              {(
                [
                  ["local", "Local time"],
                  ["utc", "UTC"],
                ] as const satisfies readonly (readonly [CronClock, string])[]
              ).map(([clock, label]) => (
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
      );
    case "once":
      return (
        <div className="field">
          <label htmlFor="schedule-at">When · in this machine's time zone</label>
          <input
            id="schedule-at"
            type="datetime-local"
            className="select-compact"
            value={cadence.at}
            onChange={(event) => onChange({ at: event.target.value })}
          />
        </div>
      );
  }
}

/** The preview beneath the cadence: the next firings, or why there are none. */
function CadencePreviewNote({ verdict }: { verdict: ReturnType<typeof cadenceVerdict> }): ReactNode {
  switch (verdict.state) {
    case "invalid":
      return <div className="banner error flush">{verdict.error}</div>;
    case "checking":
      return <span className="faint">Checking the cadence…</span>;
    case "unchecked":
      return (
        <span className="faint">
          The cadence could not be checked here ({verdict.error}). The runtime checks it again when
          the schedule is created.
        </span>
      );
    case "valid":
      return (
        <>
          <p className="muted">
            <span className="mono">{verdict.description}</span> · {verdict.heading}
          </p>
          <div className="inline" role="list" aria-label="Next firings">
            {verdict.firings.map((firing) => (
              <span key={firing} role="listitem" className="tag">
                {firing}
              </span>
            ))}
          </div>
        </>
      );
  }
}
