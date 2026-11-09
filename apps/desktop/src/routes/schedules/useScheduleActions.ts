import { useCallback, useEffect, useRef, useState } from "react";

import type { ScheduleView } from "../../bindings/ScheduleView";
import { api, describeError } from "../../sdk/client";
import { relative } from "./timeModel";

/**
 * How long a schedule just created keeps its highlight, so the eye can find
 * the new card in the list before it settles in among the others.
 */
const FRESH_MS = 2_400;

/** A change to one schedule that is waiting on the runtime. */
export type ScheduleChange = "pause" | "resume" | "delete";

/** The schedule a change is in flight for, and which change it is. */
export interface PendingChange {
  id: string;
  change: ScheduleChange;
}

/**
 * The changes the screen makes to its schedules, and what it says about them.
 *
 * One change at a time: `busy` names the schedule being changed and how, and
 * every other card's controls wait for it, which keeps the announcements in
 * the order the changes were made. The card being changed needs to know which
 * change it is, so a delete in flight is not drawn as a pause. Each change is
 * announced in words once the runtime has answered, and a failure is kept for
 * the screen to show.
 */
export function useScheduleActions(refresh: () => void, running: boolean | undefined) {
  const [busy, setBusy] = useState<PendingChange | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [fresh, setFresh] = useState<string | null>(null);

  const freshTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (freshTimer.current !== null) window.clearTimeout(freshTimer.current);
    },
    [],
  );

  const setPaused = useCallback(
    async (schedule: ScheduleView, paused: boolean) => {
      setBusy({ id: schedule.id, change: paused ? "pause" : "resume" });
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

  /** Delete a schedule; resolves to whether it was deleted. */
  const remove = useCallback(
    async (schedule: ScheduleView): Promise<boolean> => {
      setBusy({ id: schedule.id, change: "delete" });
      setActionError(null);
      try {
        await api.deleteSchedule(schedule.id);
        setAnnouncement(`Deleted ${schedule.name}. The tasks it created are kept.`);
        refresh();
        return true;
      } catch (failure) {
        setActionError(`${schedule.name} could not be deleted: ${describeError(failure)}`);
        return false;
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  const created = useCallback(
    (schedule: ScheduleView) => {
      let said = `Created ${schedule.name}.`;
      if (schedule.next_run_at) {
        said += ` It first fires ${relative(schedule.next_run_at, Date.now())}`;
        said += running === false ? ", if the scheduler is on by then." : ".";
      }
      setAnnouncement(said);
      setFresh(schedule.id);
      if (freshTimer.current !== null) window.clearTimeout(freshTimer.current);
      freshTimer.current = window.setTimeout(() => setFresh(null), FRESH_MS);
      refresh();
    },
    [refresh, running],
  );

  return { busy, actionError, announcement, fresh, setPaused, remove, created };
}
