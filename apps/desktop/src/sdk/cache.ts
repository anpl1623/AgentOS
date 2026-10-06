/**
 * Last-known answers, so returning to a screen does not start from a spinner.
 *
 * Everything here lives in memory for the life of the process. A cached answer
 * is only ever a starting point: the screen still loads, and shows the cached
 * value as `refreshing` until the runtime answers. Nothing is cached that the
 * process did not itself read from the runtime.
 *
 * The pending-approval list is deliberately not cached. `approvals.ts` owns
 * that queue, and two copies of it is how they come to disagree about whether
 * an agent is waiting.
 */

import { type RefObject, useEffect, useLayoutEffect } from "react";

import { type Async, useAsync } from "./useAsync";

/** How old a cached answer may be and still be shown while reloading. */
export const CACHE_FRESH_MS = 30_000;

/**
 * How many answers are remembered.
 *
 * Keys include arguments, so browsing many agents or traces would otherwise
 * grow the map for as long as the window stays open.
 */
export const CACHE_ENTRIES = 64;

interface Entry {
  value: unknown;
  at: number;
}

const entries = new Map<string, Entry>();

function clock(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

/**
 * The key for a command and its arguments.
 *
 * Keys are the command plus its arguments, so `get_agent` for one agent never
 * seeds the screen for another.
 */
export function cacheKey(command: string, ...args: readonly unknown[]): string {
  return JSON.stringify([command, ...args]);
}

/** A cached answer, if one is younger than {@link CACHE_FRESH_MS}. */
export function readCache<T>(key: string, now = clock()): T | null {
  const entry = entries.get(key);
  if (entry === undefined || now - entry.at > CACHE_FRESH_MS) return null;
  return entry.value as T;
}

/** Remember an answer, evicting the least recently written beyond the limit. */
export function writeCache(key: string, value: unknown, now = clock()): void {
  entries.delete(key);
  entries.set(key, { value, at: now });
  while (entries.size > CACHE_ENTRIES) {
    const oldest = entries.keys().next();
    if (oldest.done === true) break;
    entries.delete(oldest.value);
  }
}

/** Forget every cached answer. */
export function clearCache(): void {
  entries.clear();
}

/**
 * {@link useAsync}, seeded from and written back to the cache under `key`.
 *
 * The key joins the dependencies, so a new key starts a new scope seeded from
 * its own entry rather than showing the previous key's data.
 */
export function useAsyncCached<T>(
  key: string,
  load: () => Promise<T>,
  deps: readonly unknown[] = [],
): Async<T> {
  return useAsync(load, [key, ...deps], {
    seed: () => readCache<T>(key),
    onSuccess: (value) => writeCache(key, value),
  });
}

const scrollPositions = new Map<string, number>();

/**
 * Remember how far an element was scrolled under `key`, and restore it.
 *
 * The caller passes the element's ref rather than this hook finding the
 * scrolling container by selector, so the shell decides which element that is.
 * The position is recorded as it changes, not on the way out, because by the
 * time a cleanup runs the next screen's content may already have shortened the
 * element and clamped its offset.
 */
export function useScrollMemory(key: string, ref: RefObject<HTMLElement | null>): void {
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    element.scrollTop = scrollPositions.get(key) ?? 0;
  }, [key, ref]);

  useEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const record = () => scrollPositions.set(key, element.scrollTop);
    element.addEventListener("scroll", record, { passive: true });
    return () => element.removeEventListener("scroll", record);
  }, [key, ref]);
}
