import type { CSSProperties } from "react";

import type { ScheduleView } from "../../bindings/ScheduleView";
import { Icon, tip } from "../../components/common";
import { cadenceIcon } from "./cadenceModel";
import { type WhenView, scheduleCard, scheduleCardId } from "./schedulesModel";
import type { ScheduleChange } from "./useScheduleActions";

/**
 * One schedule, as a card: what it does, when it fires in words, a countdown
 * to the next firing, when it last fired, and the controls that change it.
 *
 * Not a pressable card: it holds its own controls, and a button cannot sit
 * inside another. The last task is reached by its own button, pausing by the
 * switch, and deleting by the bin, which asks first.
 *
 * The control whose change is in flight stays focusable while it waits, and
 * ignores presses, so focus is not dropped from under the person who pressed
 * it; every other control on the screen is disabled until the runtime answers.
 */
export function ScheduleCard({
  schedule,
  now,
  running,
  change,
  locked,
  fresh,
  onPause,
  onDelete,
  onLastTask,
}: {
  schedule: ScheduleView;
  now: number;
  /** Whether the scheduler is ticking; `null` when that could not be read. */
  running: boolean | null;
  /** The change in flight for this schedule, if any. */
  change: ScheduleChange | null;
  /** A change is in flight somewhere; one change at a time keeps the announcements in order. */
  locked: boolean;
  /** Just created, so it is lit for a moment to be found. */
  fresh: boolean;
  onPause: (paused: boolean) => void;
  onDelete: () => void;
  onLastTask: () => void;
}) {
  const card = scheduleCard(schedule, now, running);
  const { row, when } = card;
  const titleId = `schedule-${schedule.id}`;
  const deleting = change === "delete";
  const noteId = `resume-note-${schedule.id}`;
  const state =
    row.action === "pause" ? "is-active" : row.action === "resume" ? "is-paused" : "is-inert";
  const className = [
    "schedule-card",
    state,
    when.tone === "warn" ? "is-overdue" : null,
    fresh ? "is-fresh" : null,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article id={scheduleCardId(schedule.id)} className={className} aria-labelledby={titleId}>
      <header className="schedule-head">
        <span className="schedule-kind" aria-hidden="true">
          <Icon name={cadenceIcon(schedule.cadence.kind)} size={18} />
        </span>
        <div className="schedule-titles">
          <h3 className="schedule-name" id={titleId}>
            {schedule.name}
          </h3>
          <p className="schedule-words">{card.words}</p>
          {card.exact ? <p className="schedule-exact">{card.exact}</p> : null}
        </div>
        {row.action ? (
          <ScheduleSwitch
            name={schedule.name}
            active={row.action === "pause"}
            busy={change === "pause" || change === "resume"}
            locked={locked}
            describedBy={row.resumeNote ? noteId : undefined}
            onToggle={() => onPause(row.action === "pause")}
          />
        ) : (
          <span className={`verdict ${row.tone}`}>
            <span className="visually-hidden">schedule </span>
            {row.label}
          </span>
        )}
      </header>

      <p className="schedule-objective prose">{schedule.objective}</p>

      <div className="schedule-when">
        <NextFiring when={when} running={running} />
        <div className="schedule-last">
          <span className="schedule-eyebrow">Last fired</span>
          <span className="schedule-last-value" title={card.lastFiredAt ?? undefined}>
            {card.lastFired}
          </span>
          {schedule.last_task_id ? (
            <button type="button" className="ghost schedule-link" onClick={onLastTask}>
              Last task<span className="visually-hidden"> of {schedule.name}</span>
              <Icon name="chevron-right" size={14} />
            </button>
          ) : null}
        </div>
      </div>

      {row.resumeNote ? (
        <p className="schedule-note" id={noteId}>
          <Icon name="info" size={14} />
          <span>{row.resumeNote}</span>
        </p>
      ) : null}

      <footer className="schedule-foot">
        <span className="schedule-agent">
          <Icon name="agents" size={14} />
          <span className="visually-hidden">Agent </span>
          {schedule.agent_name}
        </span>
        <button
          type="button"
          className="icon-button schedule-delete"
          disabled={locked && !deleting}
          aria-disabled={deleting || undefined}
          aria-busy={deleting || undefined}
          onClick={deleting ? undefined : onDelete}
          {...tip(`Delete ${schedule.name}`, "top")}
        >
          <Icon name="trash" size={16} />
        </button>
      </footer>
    </article>
  );
}

/**
 * The countdown, or the words that stand in for one.
 *
 * The bar beneath it is how far through its wait the schedule is. It carries
 * a moving sheen only while the scheduler is ticking: a wait that nothing is
 * watching is drawn still, in a quiet fill.
 */
function NextFiring({ when, running }: { when: WhenView; running: boolean | null }) {
  return (
    <div className={`schedule-next tone-${when.tone}`}>
      <span className="schedule-eyebrow">{when.label}</span>
      {when.figure ? <span className="schedule-figure">{when.figure}</span> : null}
      {when.at ? <span className="schedule-at">{when.at}</span> : null}
      {when.caption ? <span className="schedule-caption">{when.caption}</span> : null}
      {when.progress === null ? null : (
        <span
          className={running === true ? "schedule-cycle is-live" : "schedule-cycle"}
          aria-hidden="true"
        >
          <span
            className={running === true ? "schedule-cycle-fill sheen" : "schedule-cycle-fill"}
            style={{ "--p": when.progress } as CSSProperties}
          />
        </span>
      )}
    </div>
  );
}

/**
 * Pause and resume, as a switch that is on while the schedule would fire.
 *
 * Drawn with the shared switch's classes, but written here because the shared
 * component cannot yet point at a description or say it is busy: Resume must
 * be described by what resuming will do, since resuming never replays what was
 * missed. The name stays the same whichever way it is set; the state is
 * announced from `aria-checked`. While the change is in flight the knob is
 * already where it is going, and returns if the runtime refuses.
 */
function ScheduleSwitch({
  name,
  active,
  busy,
  locked,
  describedBy,
  onToggle,
}: {
  name: string;
  active: boolean;
  /** Its own change is in flight: it stays focusable, and ignores presses. */
  busy: boolean;
  /** Some change is in flight. */
  locked: boolean;
  describedBy: string | undefined;
  onToggle: () => void;
}) {
  const checked = busy ? !active : active;
  const word = busy ? (active ? "Pausing…" : "Resuming…") : active ? "Active" : "Paused";
  return (
    <button
      type="button"
      role="switch"
      className="switch schedule-switch"
      aria-checked={checked}
      aria-label={`${name} active`}
      aria-describedby={describedBy}
      aria-busy={busy || undefined}
      aria-disabled={busy || undefined}
      data-tip={active ? "Pause" : "Resume"}
      disabled={locked && !busy}
      onClick={busy ? undefined : onToggle}
    >
      <span className="switch-label" aria-hidden="true">
        {word}
      </span>
      <span className="switch-track" aria-hidden="true">
        <span className="switch-knob" />
      </span>
    </button>
  );
}
