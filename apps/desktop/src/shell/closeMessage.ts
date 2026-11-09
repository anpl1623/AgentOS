/**
 * What the close guard says, as a pure function of what the runtime found.
 */

import type { CloseGuard } from "../bindings/CloseGuard";

/** What closing will stop, one paragraph per thing that is live. */
export function closeMessage(guard: CloseGuard, failure: string | null): string {
  const runs = (n: number) => (n === 1 ? "1 run" : `${n} runs`);
  const paragraphs: string[] = [];
  if (guard.live_runs > 0) {
    paragraphs.push(
      `${runs(guard.live_runs)} will be stopped and recorded as cancelled. A run that ` +
        "does not stop in time is recorded as failed the next time AgentOS opens.",
    );
  }
  if (guard.pending_approvals > 0) {
    paragraphs.push(
      guard.pending_approvals === 1
        ? "1 approval is waiting on you. It can no longer be answered once its run stops."
        : `${guard.pending_approvals} approvals are waiting on you. They can no longer be ` +
            "answered once their runs stop.",
    );
  }
  if (guard.scheduler_running) {
    paragraphs.push(
      "The scheduler stops. Schedules do not fire and queued tasks do not start until " +
        "AgentOS is opened again; it starts again on its own then.",
    );
  }
  if (failure !== null) paragraphs.push(`The window could not be closed: ${failure}`);
  return paragraphs.join("\n\n");
}
