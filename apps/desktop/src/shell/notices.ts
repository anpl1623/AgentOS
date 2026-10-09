/**
 * What the shell raises as an alert, and what an alert offers to do.
 *
 * The alert host is for things that are broken or need attention and have no
 * other surface. A pending approval is not one of them: it is already the nav
 * badge, the dashboard, the approvals screen, the dock badge and a window
 * attention request, and a fifth copy is noise. Two things are: a run that
 * failed while the operator was looking somewhere else, and a store that could
 * not reach the runtime. Nothing is raised on success.
 *
 * The rules are pure so tests can hold them.
 */

import type { EventView } from "../bindings/EventView";
import type { Route } from "../routes/route";
import type { AlertInput } from "../sdk/alerts";

/**
 * The activity kind of a failed run, as `AgentEvent::TaskFailed` serialises it
 * in `crates/agentos-core/src/event.rs`.
 */
export const RUN_FAILED_KIND = "agent.task.failed";

const RUN_FAILED_PREFIX = "run-failed:";

/**
 * How much of a failure's reason a card shows. A corner card is a pointer to
 * the run, whose trace has the reason in full.
 */
export const REASON_LIMIT = 240;

/**
 * The alert for a streamed activity event, or `null` when it warrants none.
 *
 * Only a failed run raises, as a warning that leaves by itself: the run is
 * over and its record is kept, so this is news, not a request. It is not
 * raised when the operator is already looking at that run's trace, where the
 * failure is on screen. Keyed by run, so a re-delivered event replaces its
 * card rather than stacking a second one.
 */
export function alertForEvent(event: EventView, route: Route): AlertInput | null {
  if (event.kind !== RUN_FAILED_KIND) return null;
  if (event.run_id !== null && route.name === "tasks" && route.runId === event.run_id) {
    return null;
  }
  const whole = event.summary.trim();
  const reason = whole.length > REASON_LIMIT ? `${whole.slice(0, REASON_LIMIT - 1)}…` : whole;
  return {
    id: event.run_id === null ? `event:${event.id}` : `${RUN_FAILED_PREFIX}${event.run_id}`,
    level: "warn",
    message: "A run failed",
    detail: reason === "" ? null : reason,
    link:
      event.run_id === null
        ? null
        : { label: "Open run", route: { name: "tasks", runId: event.run_id } },
    sticky: false,
  };
}

/** The stores that can report a failure to reach the runtime. */
export type StoreName = "approvals" | "events";

/** The id under which a store's failure is shown, so recovery can dismiss it. */
export function storeAlertId(store: StoreName): string {
  return `store:${store}`;
}

const STORE_TITLES: Record<StoreName, string> = {
  approvals: "Could not check for approvals; the count shown may be out of date",
  events: "Could not read the activity history",
};

/** The alert for a store's failure. Errors are kept until dismissed or recovered from. */
export function alertForStoreError(store: StoreName, message: string): AlertInput {
  return { id: storeAlertId(store), level: "error", message: STORE_TITLES[store], detail: message };
}
