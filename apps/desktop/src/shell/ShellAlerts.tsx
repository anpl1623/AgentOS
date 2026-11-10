import { useEffect } from "react";

import type { EventView } from "../bindings/EventView";
import type { Route } from "../routes/route";
import { dismiss, raise } from "../sdk/alerts";
import { usePendingApprovals } from "../sdk/approvals";
import { events } from "../sdk/client";
import { useEvents } from "../sdk/eventStream";
import { useLive } from "../sdk/live";
import { type StoreName, alertForEvent, alertForStoreError, storeAlertId } from "./notices";

/**
 * Raises a failed run, as it is streamed. Only live events raise: history read
 * on start is the record, not news.
 */
export function RunFailureAlerts({ route }: { route: Route }) {
  useLive<EventView>(events.activity, (event) => {
    const alert = alertForEvent(event, route);
    if (alert !== null) raise(alert);
  });
  return null;
}

/** Shows a store's failure to reach the runtime, and takes it down on recovery. */
export function StoreErrorAlerts() {
  useStoreAlert("approvals", usePendingApprovals().error);
  useStoreAlert("events", useEvents().error);
  return null;
}

function useStoreAlert(store: StoreName, error: string | null): void {
  // Raised once per distinct failure: a person who dismissed it is not shown
  // the same words again on every failed poll.
  useEffect(() => {
    if (error === null) dismiss(storeAlertId(store));
    else raise(alertForStoreError(store, error));
  }, [store, error]);
}
