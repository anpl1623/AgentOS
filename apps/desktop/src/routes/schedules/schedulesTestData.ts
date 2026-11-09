/**
 * Builders shared by this folder's model tests. Imported only by `*.test.ts`
 * files, so nothing here reaches the application's bundle.
 */

import type { CadencePreview } from "../../bindings/CadencePreview";
import type { ScheduleView } from "../../bindings/ScheduleView";
import type { SchedulerView } from "../../bindings/SchedulerView";
import { type CadenceForm, type ScheduleForm, blankForm } from "./formModel";

// Built from local fields, so every assertion below holds in any time zone the
// tests happen to run in.
export const NOW = new Date(2026, 9, 6, 12, 0, 0).getTime();
export const minutes = (n: number) => new Date(NOW + n * 60_000).toISOString();

export function cadence(fields: Partial<CadenceForm> = {}): CadenceForm {
  return { ...blankForm(NOW).cadence, ...fields };
}

export function form(fields: Partial<ScheduleForm> = {}): ScheduleForm {
  return {
    ...blankForm(NOW),
    agentId: "agent-1",
    name: "follow-ups",
    objective: "Draft the overdue follow-ups.",
    ...fields,
  };
}

export function schedule(fields: Partial<ScheduleView> = {}): ScheduleView {
  return {
    id: "s-1",
    name: "follow-ups",
    agent_id: "agent-1",
    agent_name: "sales",
    objective: "Draft the overdue follow-ups.",
    cadence: {
      kind: "every",
      seconds: 3600,
      expression: null,
      clock: null,
      description: "every 3600s",
    },
    status: "active",
    next_run_at: minutes(90),
    last_run_at: null,
    last_task_id: null,
    created_at: minutes(-600),
    ...fields,
  };
}

export function preview(next_runs: string[], fields: Partial<CadencePreview> = {}): CadencePreview {
  return { valid: true, error: null, description: "every 3600s", next_runs, ...fields };
}

export function scheduler(fields: Partial<SchedulerView> = {}): SchedulerView {
  return {
    running: true,
    tick_seconds: 30,
    max_concurrent_runs: 1,
    started_at: minutes(-120),
    error: null,
    active_schedules: 1,
    next_fire_at: minutes(90),
    overdue: 0,
    runnable_tasks: 0,
    unreachable_tasks: 0,
    ...fields,
  };
}
