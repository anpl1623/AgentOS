/**
 * What the task list decides, apart from how it draws it.
 *
 * The list answers two questions: how did the work that ran go, and what is
 * still waiting to start and why. The second is where the screen can most
 * easily say more than it knows — a queued task looks the same whether a tick
 * will start it in a moment or nothing ever will — so every rule that chooses
 * the words for a queued task lives here, held by a test.
 */

import type { SchedulerView } from "../bindings/SchedulerView";
import type { TaskNodeView } from "../bindings/TaskNodeView";
import type { TaskSummary } from "../bindings/TaskSummary";
import { clock } from "../sdk/format";

// ---------------------------------------------------------------------------
// The list and its toolbar
// ---------------------------------------------------------------------------

/** How many tasks the list asks for before `Load 200` is pressed. */
export const SHALLOW_LOAD = 50;

/** How many tasks `Load 200` asks for. */
export const DEEP_LOAD = 200;

/** What the operator has narrowed the list to. Every field is `""` when unused. */
export interface TaskFilter {
  /** Matched case-insensitively against the objective and the agent's name. */
  text: string;
  /** One agent, by name. */
  agent: string;
  /** One task status. */
  status: string;
}

/** An unnarrowed filter. */
export const NO_FILTER: TaskFilter = { text: "", agent: "", status: "" };

/** Whether any part of the filter is narrowing the list. */
export function isFiltering(filter: TaskFilter): boolean {
  return filter.text.trim() !== "" || filter.agent !== "" || filter.status !== "";
}

/** The tasks a filter keeps, in the order given. */
export function filterTasks(tasks: readonly TaskSummary[], filter: TaskFilter): TaskSummary[] {
  const needle = filter.text.trim().toLowerCase();
  return tasks.filter(
    (task) =>
      (filter.agent === "" || task.agent_name === filter.agent) &&
      (filter.status === "" || task.status === filter.status) &&
      (needle === "" ||
        task.objective.toLowerCase().includes(needle) ||
        task.agent_name.toLowerCase().includes(needle)),
  );
}

/**
 * The choices a toolbar select offers: every distinct value in the loaded
 * page, sorted.
 *
 * A chosen value that the page no longer holds stays listed, so the select
 * never shows a different choice from the one narrowing the list; without it,
 * the select would fall back to its first option while the list stayed
 * filtered by something no longer on screen.
 */
export function distinctOptions(values: readonly string[], selected: string): string[] {
  const options = new Set(values);
  if (selected !== "") options.add(selected);
  return [...options].sort();
}

/** `{shown} of {total}` while filtering, nothing otherwise. */
export function countNote(shown: number, total: number, filtering: boolean): string | null {
  return filtering ? `${shown} of ${total}` : null;
}

/** What the list says when it is empty, which depends on why. */
export function emptyListMessage(filtering: boolean): string {
  return filtering ? "No loaded task matches this filter." : "Nothing has been run yet.";
}

/**
 * `attempt {n}` for a task whose latest attempt is not its first.
 *
 * The first attempt says nothing: every task has one, and a meta line that
 * always reads "attempt 1" teaches the eye to skip the slot that, on a retried
 * task, carries the one fact worth seeing.
 */
export function attemptNote(task: TaskSummary): string | null {
  const attempt = task.latest_run?.attempt ?? 0;
  return attempt > 1 ? `attempt ${attempt}` : null;
}

/**
 * Whether the row also shows the task's status beside `never ran`.
 *
 * A pending or blocked task that never ran is fully described by `never ran`,
 * and the Queued panel says why. A failed or cancelled one is not: it ended
 * without running, and that ending is the point of the row.
 */
export function showsStatusWithoutRun(task: TaskSummary): boolean {
  return task.status !== "pending" && task.status !== "blocked";
}

// ---------------------------------------------------------------------------
// Queued work
// ---------------------------------------------------------------------------

/** How many characters of a blocker's objective a tag chip shows. */
export const BLOCKER_CHARS = 32;

/** Text cut to `max` characters, marked with an ellipsis when it was cut. */
export function truncate(text: string, max: number): string {
  const trimmed = text.trim();
  if (trimmed.length <= max) return trimmed;
  return `${trimmed.slice(0, max - 1).trimEnd()}…`;
}

/** A task another is waiting for, as its tag chip shows it. */
export interface Blocker {
  id: string;
  /** The blocker's objective, cut short; its id when it is outside the graph read. */
  label: string;
  /** The full objective, or the id, for the chip's `title`. */
  full: string;
}

/**
 * Why a queued task has not started.
 *
 * - `ready`: a scheduler tick would start it now.
 * - `unreachable`: something it waits for failed or was cancelled, so it never will.
 * - `waiting`: neither; it is waiting for a moment or for its blockers to succeed.
 */
export type QueueState = "ready" | "unreachable" | "waiting";

/** One row of the Queued panel. */
export interface QueueRow {
  id: string;
  objective: string;
  agent: string;
  state: QueueState;
  waitsFor: Blocker[];
  /** When the task was queued for a moment, that moment. */
  heldUntil: string | null;
  /** For an unreachable task, the failed dependency's objective, or its id. */
  culprit: string | null;
}

/** Whether a node is queued work: never run and not yet given up on. */
export function isQueued(node: TaskNodeView): boolean {
  return node.status === "pending" || node.status === "blocked";
}

/**
 * The Queued panel's rows, in the graph's order.
 *
 * A blocker is named by its objective when the graph read includes it and by
 * its id otherwise. The graph is a recent window, so a task can wait on one
 * older than the window holds; a chip with the id is less friendly than one
 * with an objective, and more honest than a chip that is missing.
 *
 * Unreachable outranks ready. The runtime should never report both, but if it
 * did, the row that claims the task will start is the one that could mislead.
 */
export function queueRows(graph: readonly TaskNodeView[]): QueueRow[] {
  const byId = new Map(graph.map((node) => [node.id, node]));
  const name = (id: string): Blocker => {
    const objective = byId.get(id)?.objective;
    return objective === undefined
      ? { id, label: truncate(id, BLOCKER_CHARS), full: id }
      : { id, label: truncate(objective, BLOCKER_CHARS), full: objective };
  };
  return graph.filter(isQueued).map((node) => ({
    id: node.id,
    objective: node.objective,
    agent: node.agent_name,
    state: node.unreachable ? "unreachable" : node.runnable ? "ready" : "waiting",
    waitsFor: node.blocked_by.map(name),
    heldUntil: node.scheduled_for,
    culprit:
      node.unreachable && node.blocked_by_failure !== null
        ? name(node.blocked_by_failure).full
        : null,
  }));
}

/**
 * `held until …` for a queued moment.
 *
 * The clock alone when the moment is today, and the date with it otherwise:
 * "held until 09:00" on a task held until next week reads as this morning.
 */
export function heldUntil(iso: string, now: number): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return `held until ${iso}`;
  const today = new Date(now);
  const sameDay =
    at.getFullYear() === today.getFullYear() &&
    at.getMonth() === today.getMonth() &&
    at.getDate() === today.getDate();
  if (sameDay) return `held until ${clock(iso)}`;
  const date = at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  return `held until ${date} ${clock(iso)}`;
}

/** Whether a node's work is still to be done: not succeeded, failed or cancelled. */
export function isIncomplete(node: TaskNodeView): boolean {
  return node.status === "pending" || node.status === "blocked" || node.status === "running";
}

/** A task offered as something to wait for. */
export interface DependencyChoice {
  id: string;
  label: string;
}

/**
 * The tasks a new or queued task can be made to wait for: incomplete ones,
 * leaving out the task itself and what it already waits for.
 *
 * A finished task is left out. Waiting for one that succeeded waits for
 * nothing, and waiting for one that failed makes the new task unreachable the
 * moment it is written. So is a task the runtime has already marked
 * unreachable: it will be cancelled rather than run, and anything waiting for
 * it would follow it. A choice that would close a cycle is not filtered
 * here: the runtime refuses it with the path, and that message says more than
 * an option that silently is not there.
 */
export function dependencyChoices(
  graph: readonly TaskNodeView[],
  forTask: TaskNodeView | null = null,
): DependencyChoice[] {
  const excluded = new Set(forTask ? [forTask.id, ...forTask.blocked_by] : []);
  return graph
    .filter((node) => isIncomplete(node) && !node.unreachable && !excluded.has(node.id))
    .map((node) => ({
      id: node.id,
      label: `${truncate(node.objective, 60)} · ${node.agent_name}`,
    }));
}

/**
 * Whether queued work will start on its own, in one sentence, or `null` when
 * it will.
 *
 * Queued work is only ever started by a scheduler tick, and the desktop's
 * scheduler is off until someone turns it on. Without this the Queue button
 * would look like a slower Run.
 */
export function schedulerNote(scheduler: SchedulerView | null): string | null {
  if (scheduler === null) return null;
  if (scheduler.running) return null;
  if (scheduler.error !== null) {
    return (
      `The scheduler stopped (${scheduler.error}), so queued tasks will not start on ` +
      "their own."
    );
  }
  return "The scheduler is off, so queued tasks will not start on their own.";
}
