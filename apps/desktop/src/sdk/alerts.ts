/**
 * Alerts: the console telling a person something they did not ask about.
 *
 * The policy is that this console speaks when something is waiting or broken
 * and stays silent otherwise, so nothing raises an alert on success. A stream
 * of confirmation toasts trains an operator to dismiss without reading, which
 * is exactly the failure the approval card is designed against; an alert that
 * appears here should be worth the second it takes to read.
 *
 * The store is a module rather than a React context so anything can raise —
 * the approval queue, the event stream, a screen — without being threaded a
 * provider. The rules that decide what stays on screen are pure functions over
 * a list, so they can be tested without a renderer.
 */

import { useSyncExternalStore } from "react";

/**
 * How loud an alert is.
 *
 * Each level is also the class that gives the card its tone — rendered as
 * `alert ${level}` against `.alert.info`, `.alert.warn` and `.alert.error` in
 * `styles/components.css` — so the two vocabularies are one. A test holds them
 * together.
 */
export type AlertLevel = "info" | "warn" | "error";

/** Every level, for code and tests that must cover them all. */
export const ALERT_LEVELS: readonly AlertLevel[] = ["info", "warn", "error"];

/** An alert on screen. */
export interface Alert {
  /** Identity. Raising again with the same id replaces this alert. */
  id: string;
  level: AlertLevel;
  message: string;
  /** Stays until dismissed; never expires and is never evicted to make room. */
  sticky: boolean;
}

/** What a caller passes to {@link raise}. */
export interface AlertInput {
  /**
   * Identity. Give one whenever the same condition can be reported twice — a
   * re-delivered event should replace its card, not stack a second one.
   */
  id?: string;
  level: AlertLevel;
  message: string;
  /** Keep until dismissed. Errors are always kept, whatever this says. */
  sticky?: boolean;
}

/**
 * How many alerts may be on screen before older ones make room.
 *
 * Only non-sticky alerts are evicted. When every alert on screen is sticky a
 * new one is shown anyway: an overfull corner is a nuisance, a condition that
 * was never shown is a hazard.
 */
export const ALERT_CAP = 4;

/** How long a non-sticky alert stays before it leaves by itself. */
export const ALERT_TTL_MS = 8000;

/**
 * Normalise an input into an alert.
 *
 * An error is forced sticky, so neither the expiry timer nor the cap can take
 * it off screen without a person having seen it.
 */
export function toAlert(input: AlertInput, fallbackId: string): Alert {
  return {
    id: input.id ?? fallbackId,
    level: input.level,
    message: input.message,
    sticky: input.level === "error" || input.sticky === true,
  };
}

/**
 * Add an alert to a list, replacing any alert with the same id in place.
 *
 * Then, while the list is over {@link ALERT_CAP}, remove the oldest non-sticky
 * alert other than the one just raised. If no such alert exists the list stays
 * over the cap.
 */
export function withAlert(list: readonly Alert[], alert: Alert, cap = ALERT_CAP): Alert[] {
  const existing = list.findIndex((each) => each.id === alert.id);
  const next =
    existing === -1
      ? [...list, alert]
      : list.map((each, index) => (index === existing ? alert : each));

  while (next.length > cap) {
    const oldest = next.findIndex((each) => !each.sticky && each.id !== alert.id);
    if (oldest === -1) break;
    next.splice(oldest, 1);
  }
  return next;
}

/** Remove an alert by id. */
export function withoutAlert(list: readonly Alert[], id: string): Alert[] {
  return list.filter((each) => each.id !== id);
}

let alerts: readonly Alert[] = [];
let counter = 0;
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const listeners = new Set<() => void>();

function publish(next: readonly Alert[]): void {
  alerts = next;
  for (const listener of listeners) listener();
}

/** Show an alert. Returns its id, for dismissing it later. */
export function raise(input: AlertInput): string {
  counter += 1;
  const alert = toAlert(input, `alert-${counter}`);

  const previous = timers.get(alert.id);
  if (previous !== undefined) clearTimeout(previous);
  timers.delete(alert.id);
  if (!alert.sticky) {
    timers.set(
      alert.id,
      setTimeout(() => dismiss(alert.id), ALERT_TTL_MS),
    );
  }

  const next = withAlert(alerts, alert);
  // Anything the cap evicted no longer needs its expiry timer.
  for (const [id, timer] of timers) {
    if (!next.some((each) => each.id === id)) {
      clearTimeout(timer);
      timers.delete(id);
    }
  }
  publish(next);
  return alert.id;
}

/** Take an alert off screen. Dismissing an alert that is not there does nothing. */
export function dismiss(id: string): void {
  const timer = timers.get(id);
  if (timer !== undefined) clearTimeout(timer);
  timers.delete(id);
  if (alerts.some((each) => each.id === id)) publish(withoutAlert(alerts, id));
}

function subscribeAlerts(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The alerts on screen, oldest first. */
export function useAlerts(): readonly Alert[] {
  return useSyncExternalStore(subscribeAlerts, () => alerts);
}
