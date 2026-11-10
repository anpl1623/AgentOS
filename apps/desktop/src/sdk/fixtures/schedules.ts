/** Schedules, the task graph, and the scheduler that would run them. */

import type { CadenceInput } from "../../bindings/CadenceInput";
import type { CadencePreview } from "../../bindings/CadencePreview";
import type { ScheduleView } from "../../bindings/ScheduleView";
import type { SchedulerView } from "../../bindings/SchedulerView";
import type { TaskNodeView } from "../../bindings/TaskNodeView";

import { agents } from "./agents";
import { minutesAgo, minutesAhead, now } from "./clock";
import { Refusal } from "./refusal";

/**
 * Standing work. One cron schedule is active, a one-off is waiting for its
 * moment, and an interval schedule is paused.
 *
 * A schedule only fires, and a queued task only starts on its own, while the
 * scheduler is running. The scheduler below is stopped, as it is on a first
 * launch: these schedules are therefore not firing, and the screens built on
 * this data have to say so.
 */
export const schedules: ScheduleView[] = [
  {
    id: "5c0e0001-aaaa-4bbb-8ccc-000000000001",
    name: "weekday-follow-ups",
    agent_id: agents[0]!.id,
    agent_name: "sales",
    objective: "Find every customer whose follow-up is overdue and draft a message for each.",
    cadence: {
      kind: "cron",
      seconds: null,
      expression: "0 9 * * 1-5",
      clock: "local",
      description: "cron `0 9 * * 1-5` (local)",
    },
    status: "active",
    next_run_at: minutesAhead(1348),
    last_run_at: minutesAgo(92),
    last_task_id: "task-0001",
    created_at: minutesAgo(20_000),
  },
  {
    id: "5c0e0001-aaaa-4bbb-8ccc-000000000002",
    name: "quarter-close-report",
    agent_id: agents[0]!.id,
    agent_name: "sales",
    objective: "Summarise the quarter's closed deals.",
    cadence: { kind: "once", seconds: null, expression: null, clock: null, description: "once" },
    status: "active",
    next_run_at: minutesAhead(8_640),
    last_run_at: null,
    last_task_id: null,
    created_at: minutesAgo(300),
  },
  {
    id: "5c0e0001-aaaa-4bbb-8ccc-000000000003",
    name: "log-sweep",
    agent_id: agents[1]!.id,
    agent_name: "ops",
    objective: "Archive the log files older than thirty days.",
    cadence: {
      kind: "every",
      seconds: 21_600,
      expression: null,
      clock: null,
      description: "every 21600s",
    },
    status: "paused",
    next_run_at: minutesAgo(30),
    last_run_at: minutesAgo(1500),
    last_task_id: "task-0003",
    created_at: minutesAgo(30_000),
  },
];

/**
 * The fan-in: two gatherers and a summariser that waits for both. One
 * gatherer failed, so the summariser can never start. With the scheduler
 * stopped, nothing has abandoned it yet; the graph still has to show it as
 * dead rather than as waiting its turn.
 */
export const graph: TaskNodeView[] = (() => {
  const gatherCrm = "task-0101";
  const gatherMail = "task-0102";
  const summarise = "task-0103";
  const node = (
    id: string,
    objective: string,
    status: string,
    blockedBy: string[],
    blocks: string[],
    minutes: number,
  ): TaskNodeView => ({
    id,
    objective,
    status,
    agent_id: agents[0]!.id,
    agent_name: "sales",
    scheduled_for: null,
    schedule_id: null,
    created_at: minutesAgo(minutes),
    blocked_by: blockedBy,
    blocks,
    runnable: false,
    unreachable: false,
    blocked_by_failure: null,
  });
  return [
    node(gatherCrm, "Collect this week's CRM changes.", "succeeded", [], [summarise], 64),
    node(gatherMail, "Collect this week's replies from the inbox.", "failed", [], [summarise], 64),
    {
      ...node(
        summarise,
        "Summarise the week for the Monday meeting.",
        "blocked",
        [gatherCrm, gatherMail],
        [],
        63,
      ),
      unreachable: true,
      blocked_by_failure: gatherMail,
    },
  ];
})();

/** A stopped scheduler, with the work it would find if it were started. */
export const scheduler: SchedulerView = {
  running: false,
  tick_seconds: 30,
  max_concurrent_runs: 1,
  started_at: null,
  error: null,
  active_schedules: schedules.filter((schedule) => schedule.status === "active").length,
  next_fire_at: schedules[0]!.next_run_at,
  overdue: 0,
  runnable_tasks: graph.filter((node) => node.runnable).length,
  unreachable_tasks: graph.filter((node) => node.unreachable).length,
};

/** The runtime's check of a cadence, close enough to build the form against. */
export function checkCadence(cadence: CadenceInput): CadencePreview {
  const kinds: Record<string, boolean> = {
    once: cadence.seconds === null && cadence.expression === null && cadence.clock === null,
    every: cadence.seconds !== null && cadence.expression === null && cadence.clock === null,
    cron: cadence.seconds === null && cadence.expression !== null,
  };
  if (!kinds[cadence.kind]) throw new Refusal("choose exactly one of once, every or cron");
  const invalid = (error: string): CadencePreview => ({
    valid: false,
    error,
    description: null,
    next_runs: [],
  });
  if (cadence.kind === "once") {
    return { valid: true, error: null, description: "once", next_runs: [] };
  }
  if (cadence.kind === "every") {
    const seconds = cadence.seconds ?? 0;
    if (seconds < 60) {
      return invalid(
        `an interval of ${seconds}s is shorter than the 60s minimum; ` +
          "each firing is a whole agent run",
      );
    }
    return {
      valid: true,
      error: null,
      description: `every ${seconds}s`,
      next_runs: [1, 2, 3, 4, 5].map((n) =>
        new Date(now.getTime() + n * seconds * 1000).toISOString(),
      ),
    };
  }
  const expression = cadence.expression ?? "";
  const fields = expression.trim().split(/\s+/).length;
  if (fields < 5 || fields > 7) {
    return invalid(`\`${expression}\` is not a cron expression: invalid number of fields`);
  }
  return {
    valid: true,
    error: null,
    description: `cron \`${expression}\` (${cadence.clock ?? "utc"})`,
    // Not a cron evaluator: an hour apart, so a preview has something to show.
    next_runs: [1, 2, 3, 4, 5].map((n) => minutesAhead(n * 60)),
  };
}

/** The scheduler switch, refusing what the runtime refuses. */
export function setSchedulerRunning(args: Record<string, unknown>): SchedulerView {
  const tick = Number(args.tickSeconds);
  const max = Number(args.maxConcurrentRuns);
  if (!(tick >= 5)) throw new Refusal("a tick shorter than 5 seconds is a busy loop");
  if (!(max >= 1)) throw new Refusal("the scheduler needs room for at least one run");
  const running = args.enabled === true;
  return {
    ...scheduler,
    running,
    tick_seconds: tick,
    max_concurrent_runs: max,
    started_at: running ? new Date().toISOString() : null,
  };
}
