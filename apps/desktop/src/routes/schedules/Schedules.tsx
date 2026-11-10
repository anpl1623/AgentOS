import { useCallback, useMemo, useRef, useState } from "react";

import type { ScheduleView } from "../../bindings/ScheduleView";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Icon, PageHeader } from "../../components/common";
import { useNow } from "../../components/hooks";
import { prefersReducedMotion } from "../../components/motion";
import { api } from "../../sdk/client";
import { useVisibleInterval } from "../../sdk/live";
import { useAsync } from "../../sdk/useAsync";
import { useNavigate } from "../../shell/router";
import { NewScheduleSection } from "./CreateSchedule";
import { ScheduleList } from "./ScheduleList";
import { SchedulerCard, ScheduleStats } from "./SchedulerOverview";
import { summarise } from "./overviewModel";
import { deleteMessage, scheduleCardId, sortSchedules } from "./schedulesModel";
import { useScheduleActions } from "./useScheduleActions";

/**
 * How often the list and the scheduler's state are re-read while visible.
 *
 * Schedules change on their own only when one fires, and the soonest change
 * a person would notice is a minute-resolution "next in 4m"; a few seconds of
 * lag says nothing wrong.
 */
const LIST_MS = 15_000;

/**
 * Standing instructions: an objective given to an agent on an interval, on a
 * cron, or once.
 *
 * The scheduler's state is read beside the list on every refresh, because a
 * list of active schedules with the scheduler off is a list of things that
 * are not happening, and the screen has to say so where the list is.
 *
 * This file only composes: the scheduler's card and the figures are in
 * `SchedulerOverview`, the cards in `ScheduleList` and `ScheduleCard`, the
 * form in `CreateSchedule` (its cadence in `CadenceFields` and
 * `CadencePreview`), the changes in `useScheduleActions`, and every decision
 * in the `*Model.ts` files beside them.
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

  const reloadSchedules = schedules.reload;
  const reloadScheduler = scheduler.reload;
  const refresh = useCallback(() => {
    reloadSchedules();
    reloadScheduler();
  }, [reloadSchedules, reloadScheduler]);

  const running = scheduler.data?.running ?? null;
  const actions = useScheduleActions(refresh, scheduler.data?.running);
  const [confirming, setConfirming] = useState<ScheduleView | null>(null);

  const list = useMemo(
    () => (schedules.data ? sortSchedules(schedules.data) : null),
    [schedules.data],
  );
  const takenNames = useMemo(() => list?.map((schedule) => schedule.name) ?? [], [list]);
  const enabled = agents.data?.filter((agent) => agent.status === "enabled") ?? null;
  // The figures hold their places while the list is first read, and give way
  // to the empty state, or to the failure, when there is nothing to count.
  const showStats = list === null ? schedules.loading : list.length > 0;

  return (
    <div className="schedules-screen">
      <PageHeader
        eyebrow="Automation"
        title="Schedules"
        subtitle="Standing instructions: an objective an agent is given on an interval, on a cron, or once."
        actions={
          <button type="button" className="primary" onClick={focusCreate}>
            <Icon name="plus" size={16} />
            New schedule
          </button>
        }
      />

      <div className="schedules-overview">
        <SchedulerCard
          status={scheduler.data}
          error={scheduler.error}
          now={now}
          openSettings={() => navigate({ name: "settings" })}
        />
        {showStats ? (
          <ScheduleStats summary={list ? summarise(list, now) : null} running={running} />
        ) : null}
      </div>

      <p className="visually-hidden" role="status">
        {actions.announcement}
      </p>

      <ScheduleList
        list={list}
        loading={schedules.loading}
        error={schedules.error}
        staleSince={schedules.stale ? loadedAt.current : null}
        actionError={actions.actionError}
        now={now}
        running={running}
        busy={actions.busy}
        fresh={actions.fresh}
        onPause={(schedule, paused) => void actions.setPaused(schedule, paused)}
        onDelete={setConfirming}
        onLastTask={() => navigate({ name: "tasks" })}
        onCreate={focusCreate}
      />

      <NewScheduleSection
        agents={enabled}
        agentsError={agents.error}
        now={now}
        takenNames={takenNames}
        onCreated={actions.created}
        openAgents={() => navigate({ name: "agents" })}
      />

      {confirming ? (
        <ConfirmDialog
          title={`Delete ${confirming.name}?`}
          message={deleteMessage(confirming)}
          confirmLabel="Delete schedule"
          cancelLabel="Keep it"
          onAnswer={(confirmed) => {
            const schedule = confirming;
            setConfirming(null);
            if (!confirmed) return;
            void actions.remove(schedule).then((deleted) => {
              if (deleted) settleFocusAfterDelete(schedule.id);
            });
          }}
        />
      ) : null}
    </div>
  );
}

/**
 * Take the person to the create form: its first text field when there is a
 * form, otherwise the section's heading, where the reason there is no form is.
 * The scroll is instant under reduced motion.
 */
function focusCreate() {
  const target =
    document.getElementById("schedule-name") ?? document.getElementById("schedules-new");
  if (target === null) return;
  target.scrollIntoView({
    block: "center",
    behavior: prefersReducedMotion() ? "auto" : "smooth",
  });
  target.focus({ preventScroll: true });
}

/**
 * After a delete, move focus to the list's heading if it would otherwise be
 * lost: it is still on the deleted card's bin, which is about to go, or it has
 * already fallen to the page. Focus the person has moved elsewhere is left
 * where they put it.
 */
function settleFocusAfterDelete(id: string) {
  const active = document.activeElement;
  const card = document.getElementById(scheduleCardId(id));
  const lost = active === null || active === document.body || (card?.contains(active) ?? false);
  if (lost) document.getElementById("schedules-list")?.focus();
}
