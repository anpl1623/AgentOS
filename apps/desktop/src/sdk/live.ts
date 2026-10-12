/**
 * Shared runtime listeners and polling that stops when nobody is looking.
 *
 * Two screens that care about the same event share one Tauri listener rather
 * than opening one each: every listener is a registration on the Rust side and
 * a callback per event, and a screen that re-subscribes on every render turns
 * one channel into a churn of listen and unlisten calls.
 *
 * Polling is gated on visibility for the same reason in the other direction. A
 * window minimised to the dock has no reader, and a command issued for nobody
 * still costs the runtime a database read that cannot be cancelled once sent.
 */

import { useEffect, useRef } from "react";

import { subscribe } from "./transport";

type Handler = (payload: unknown) => void;

interface Channel {
  handlers: Set<Handler>;
  unlisten: Promise<() => void>;
}

const channels = new Map<string, Channel>();

/**
 * Listen to a runtime event on the shared channel for its name.
 *
 * The first listener opens the channel and the last one to leave closes it.
 * Returns the function that removes this handler.
 */
export function listenLive<T>(event: string, handler: (payload: T) => void): () => void {
  let channel = channels.get(event);
  if (!channel) {
    const handlers = new Set<Handler>();
    const unlisten = subscribe<unknown>(event, (payload) => {
      // Copied first so a handler that unsubscribes during delivery does not
      // change the set being walked; isolated so one faulty handler cannot
      // starve the others of an event they are waiting for.
      for (const each of [...handlers]) {
        try {
          each(payload);
        } catch (failure) {
          console.error(`A handler for ${event} failed`, failure);
        }
      }
    }).catch((failure: unknown) => {
      console.error(`Could not listen for ${event}`, failure);
      return () => {};
    });
    channel = { handlers, unlisten };
    channels.set(event, channel);
  }

  const joined = channel;
  const wrapped: Handler = (payload) => handler(payload as T);
  joined.handlers.add(wrapped);

  return () => {
    joined.handlers.delete(wrapped);
    if (joined.handlers.size === 0 && channels.get(event) === joined) {
      channels.delete(event);
      void joined.unlisten.then((unlisten) => unlisten());
    }
  };
}

/**
 * Listen to a runtime event for as long as the component is mounted.
 *
 * The handler is read through a ref, so a caller may pass an inline arrow
 * without re-registering on every render — which is what would otherwise make
 * the shared channel open and close continuously.
 */
export function useLive<T>(event: string, handler: (payload: T) => void): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => listenLive<T>(event, (payload) => handlerRef.current(payload)), [event]);
}

/** What {@link startVisibleInterval} needs from its surroundings. */
export interface VisibilityHost {
  /** Whether a person could be looking at the window now. */
  isVisible: () => boolean;
  /** Call back whenever visibility changes. Returns the unsubscribe function. */
  onVisibilityChange: (callback: () => void) => () => void;
  /** Call back whenever the window gains focus. Returns the unsubscribe function. */
  onFocus: (callback: () => void) => () => void;
  /** A monotonic clock in milliseconds. */
  now: () => number;
}

/**
 * How close to another fire a focus event is treated as the same return.
 *
 * Restoring a minimised window raises both a visibility change and a focus
 * event within a few milliseconds; they describe one return, not two.
 */
export const FOCUS_DEBOUNCE_MS = 1000;

function documentHost(): VisibilityHost {
  const hasDocument = typeof document !== "undefined";
  const hasWindow = typeof window !== "undefined";
  return {
    isVisible: () => !hasDocument || document.visibilityState === "visible",
    onVisibilityChange: (callback) => {
      if (!hasDocument) return () => {};
      document.addEventListener("visibilitychange", callback);
      return () => document.removeEventListener("visibilitychange", callback);
    },
    onFocus: (callback) => {
      if (!hasWindow) return () => {};
      window.addEventListener("focus", callback);
      return () => window.removeEventListener("focus", callback);
    },
    now: () => (typeof performance !== "undefined" ? performance.now() : Date.now()),
  };
}

/**
 * Run `fn` every `everyMs` while the window is visible.
 *
 * Nothing runs while hidden. Becoming visible runs `fn` at once and restarts
 * the period from there, because whatever the screen shows went unrefreshed
 * for as long as it was hidden. Focus also runs it, unless something else ran
 * it within {@link FOCUS_DEBOUNCE_MS}. Starting does not run it: the screen
 * that starts a poll has almost always just loaded the same thing.
 *
 * Returns the function that stops it.
 */
export function startVisibleInterval(
  fn: () => void,
  everyMs: number,
  host: VisibilityHost = documentHost(),
): () => void {
  let timer: ReturnType<typeof setInterval> | null = null;
  let lastFired = Number.NEGATIVE_INFINITY;

  const fire = () => {
    lastFired = host.now();
    fn();
  };
  const arm = () => {
    if (timer === null) timer = setInterval(fire, everyMs);
  };
  const disarm = () => {
    if (timer !== null) clearInterval(timer);
    timer = null;
  };

  const stopVisibility = host.onVisibilityChange(() => {
    if (host.isVisible()) {
      disarm();
      fire();
      arm();
    } else {
      disarm();
    }
  });
  const stopFocus = host.onFocus(() => {
    if (!host.isVisible() || host.now() - lastFired < FOCUS_DEBOUNCE_MS) return;
    disarm();
    fire();
    arm();
  });

  if (host.isVisible()) arm();

  return () => {
    disarm();
    stopVisibility();
    stopFocus();
  };
}

/**
 * Re-run something on an interval while the window is visible.
 *
 * Live runs change without the interface asking, and the activity stream only
 * carries events — not the derived state a screen shows. Polling keeps the two
 * from drifting without every screen inventing its own refresh. The callback
 * is read through a ref, so an inline arrow does not restart the timer.
 */
export function useVisibleInterval(fn: () => void, everyMs = 4000): void {
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => startVisibleInterval(() => fnRef.current(), everyMs), [everyMs]);
}

/**
 * The previous name of {@link useVisibleInterval}.
 *
 * @deprecated Use `useVisibleInterval`; this alias exists so screens can move
 * across one at a time.
 */
export function useRefresh(reload: () => void, everyMs = 4000): void {
  useVisibleInterval(reload, everyMs);
}
