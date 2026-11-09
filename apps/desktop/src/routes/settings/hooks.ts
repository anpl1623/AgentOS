/**
 * The state more than one part of the screen reads: the scheduler's status,
 * which its panel and the overview tile both show, and the last verification
 * of the audit chain, which the header's button, the tile and the audit panel
 * all share.
 */

import { type RefObject, useCallback, useEffect, useRef, useState } from "react";

import type { SchedulerView } from "../../bindings/SchedulerView";
import { api, describeError } from "../../sdk/client";
import { useVisibleInterval } from "../../sdk/live";
import { type Async, useAsync } from "../../sdk/useAsync";
import { type AuditFindings, auditFindings } from "./auditModel";

/**
 * How often the scheduler's counts are re-read while the screen is visible.
 *
 * The scheduler can stop on its own, and schedules come due while the screen
 * is open; a panel that only read once would keep saying "running" over a
 * scheduler that is not.
 */
const SCHEDULER_MS = 10_000;

/** The scheduler as the screen shows it. */
export interface SchedulerState {
  /** The status as last read, with its error and a way to read it again. */
  status: Async<SchedulerView>;
  /** What to draw: the switch's own answer if there is one, else the status. */
  view: SchedulerView | null;
  /** Hold the switch's answer until the next read of the status replaces it. */
  answered: (view: SchedulerView) => void;
}

/**
 * The scheduler's status, re-read while the screen is visible.
 *
 * The switch's own answer is shown until the next read of the status, which
 * it then asks for. Without it the panel would read the old state for the
 * moment between the two.
 */
export function useScheduler(): SchedulerState {
  const status = useAsync(() => api.schedulerStatus(), []);
  useVisibleInterval(status.reload, SCHEDULER_MS);
  const [answer, setAnswer] = useState<SchedulerView | null>(null);
  useEffect(() => setAnswer(null), [status.data]);
  return { status, view: answer ?? status.data, answered: setAnswer };
}

/**
 * The last full verification of the audit chain, kept for the life of the
 * window rather than the life of the screen.
 *
 * The line under the button answers "have I checked at all since I opened
 * this?", and leaving Settings for a moment must not reset that to "no".
 * Held in memory only: a result read back from disk after a restart would be
 * a claim about a chain that may since have changed.
 */
let lastVerification: AuditFindings | null = null;

/** A verification of the audit chain, and the button's state. */
export interface AuditState {
  last: AuditFindings | null;
  verifying: boolean;
  error: string | null;
  verify: () => void;
}

export function useAuditVerification(): AuditState {
  const [last, setLast] = useState<AuditFindings | null>(() => lastVerification);
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(async () => {
    setVerifying(true);
    setError(null);
    try {
      const result = auditFindings(new Date().toISOString(), await api.verifyAudit());
      lastVerification = result;
      setLast(result);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setVerifying(false);
    }
  }, []);

  const verify = useCallback(() => {
    // Two buttons start the same check; a second press while one runs would
    // only race the first to the same answer.
    if (!verifying) void run();
  }, [run, verifying]);

  return { last, verifying, error, verify };
}

/**
 * Put focus back on the control that opened a form, once the form closes
 * with focus inside it.
 *
 * A form closes by its own Cancel, or by being sent; either way the button
 * that was pressed leaves the document, and focus would fall to the page's
 * start, so the next Tab would begin at the top of the window. Focus is moved
 * only when it has fallen like that: a form closed from its toggle leaves
 * focus on the toggle already, and focus the person has since moved
 * elsewhere is left where they put it.
 */
export function useReturnFocus(open: boolean, opener: RefObject<HTMLElement | null>): void {
  const wasOpen = useRef(open);
  useEffect(() => {
    const closed = wasOpen.current && !open;
    wasOpen.current = open;
    if (!closed) return;
    const active = document.activeElement;
    if (active === null || active === document.body) opener.current?.focus();
  }, [open, opener]);
}
