import { useState } from "react";

import type { CloseGuard } from "../bindings/CloseGuard";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { api, describeError, events } from "../sdk/client";
import { useLive } from "../sdk/live";
import { closeMessage } from "./closeMessage";

/**
 * Asks before the window closes on live work.
 *
 * The runtime holds a close of the main window while runs are live, the
 * scheduler is on or approvals are waiting, and sends what it found. Closing
 * stops all of it, and none of that is visible from a close button, so the
 * question names each thing it will stop. Keeping the window open is the
 * default answer; only "Close anyway" asks the runtime to close for real.
 */
export function CloseGuardHost() {
  const [guard, setGuard] = useState<CloseGuard | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  useLive<CloseGuard>(events.closeRequested, (next) => {
    // Acknowledged before anything is drawn: the runtime closes a window
    // whose interface does not answer, and this is the answer.
    api.acknowledgeClose().catch(() => undefined);
    setFailure(null);
    setGuard(next);
  });

  if (guard === null) return null;
  return (
    <ConfirmDialog
      title="Close AgentOS?"
      message={closeMessage(guard, failure)}
      confirmLabel="Close anyway"
      cancelLabel="Keep open"
      onAnswer={(close) => {
        if (!close) {
          setGuard(null);
          return;
        }
        api.confirmClose().catch((error: unknown) => setFailure(describeError(error)));
      }}
    />
  );
}
