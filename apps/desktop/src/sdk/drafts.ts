/**
 * Unfinished input, and the guard that stops it being walked away from.
 *
 * Drafts are held in memory for the life of the process and never written to
 * `localStorage` or anywhere else. A draft can be a policy document, an
 * objective, or content pasted from somewhere sensitive, and none of that
 * should land on disk anywhere but the runtime's own storage, where it is
 * governed like everything else the runtime keeps. The provider-key field in
 * Settings must never use {@link useDraft}: a secret held in a draft outlives
 * the moment it was needed, which is precisely what a key field must not do.
 *
 * The navigation guard is a registry rather than a component, so the shell can
 * ask one question — may I leave? — without knowing which screens have
 * something unsaved.
 */

import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";

const drafts = new Map<string, unknown>();
const draftListeners = new Map<string, Set<() => void>>();

function notifyDraft(key: string): void {
  for (const listener of draftListeners.get(key) ?? []) listener();
}

/** Read a draft, if one is held under `key`. */
export function readDraft<T>(key: string): T | undefined {
  return drafts.get(key) as T | undefined;
}

/** Hold a draft under `key`. */
export function writeDraft<T>(key: string, value: T): void {
  drafts.set(key, value);
  notifyDraft(key);
}

/** Discard the draft under `key`. */
export function clearDraft(key: string): void {
  if (drafts.delete(key)) notifyDraft(key);
}

/** The starting value a mounted {@link useDraft} holds, and the key it belongs to. */
export interface LatchedInitial<T> {
  key: string;
  initial: T;
}

/**
 * Which starting value applies for `key`.
 *
 * `initial` is read once per key, like `useState`'s is once per mount: kept
 * while the key stays the same, so a caller passing a fresh object each render
 * does not reset anything, and taken afresh when the key changes. A screen
 * that stays mounted across a route change — the policy of agent A, then of
 * agent B — must not show A's document as B's draft, where saving it would
 * write A's policy onto B.
 */
export function latchInitial<T>(
  latched: LatchedInitial<T> | null,
  key: string,
  initial: T,
): LatchedInitial<T> {
  return latched !== null && latched.key === key ? latched : { key, initial };
}

/**
 * A value that survives the component unmounting, keyed by `key`.
 *
 * Returns the value, a setter, and a function that discards the draft so the
 * next mount starts from `initial` again. `initial` is read once per key: on
 * first render, and again whenever `key` changes (see {@link latchInitial}).
 */
export function useDraft<T>(
  key: string,
  initial: T,
): [T, (next: T | ((previous: T) => T)) => void, () => void] {
  const initialRef = useRef<LatchedInitial<T> | null>(null);
  // Written during render, which is safe here because it is idempotent: the
  // same key and a latched value always produce the same answer.
  initialRef.current = latchInitial(initialRef.current, key, initial);
  const fallback = (): T =>
    initialRef.current !== null && initialRef.current.key === key
      ? initialRef.current.initial
      : initial;

  const value = useSyncExternalStore(
    useCallback(
      (listener: () => void) => {
        let listeners = draftListeners.get(key);
        if (listeners === undefined) {
          listeners = new Set();
          draftListeners.set(key, listeners);
        }
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
          if (listeners.size === 0) draftListeners.delete(key);
        };
      },
      [key],
    ),
    () => (drafts.has(key) ? (drafts.get(key) as T) : fallback()),
  );

  const set = useCallback(
    (next: T | ((previous: T) => T)) => {
      const previous = drafts.has(key) ? (drafts.get(key) as T) : fallback();
      writeDraft(
        key,
        typeof next === "function" ? (next as (previous: T) => T)(previous) : next,
      );
    },
    [key],
  );
  const clear = useCallback(() => clearDraft(key), [key]);

  return [value, set, clear];
}

/**
 * A navigation guard: returns what would be lost by leaving, or `null` if
 * nothing would be.
 */
export type LeaveGuard = () => string | null;

/** Asks a person whether to leave anyway. */
export type LeaveConfirmer = (message: string) => boolean | Promise<boolean>;

const guards = new Set<LeaveGuard>();
let confirmer: LeaveConfirmer | null = null;

/** Register a guard. Returns the function that removes it. */
export function registerGuard(guard: LeaveGuard): () => void {
  guards.add(guard);
  return () => {
    guards.delete(guard);
  };
}

/**
 * Choose how {@link confirmLeave} asks.
 *
 * The shell plugs in an in-app dialog here, because Tauri's webview is not
 * guaranteed to show `window.confirm` at all, and a prompt that never appears
 * is a question nobody can answer. Passing `null` returns to the default.
 */
export function setLeaveConfirmer(next: LeaveConfirmer | null): void {
  confirmer = next;
}

/**
 * Whether it is all right to navigate away.
 *
 * Resolves `true` at once when no guard has anything to lose. Otherwise asks
 * once, with every guard's message, through the confirmer the shell installed
 * or `window.confirm` if there is none. With no way to ask at all it resolves
 * `false`: leaving without asking is the one answer that cannot be taken back.
 */
export async function confirmLeave(confirm: LeaveConfirmer | null = confirmer): Promise<boolean> {
  const messages = [...new Set([...guards].map((guard) => guard()).filter(isMessage))];
  if (messages.length === 0) return true;

  const ask = confirm ?? defaultConfirmer();
  if (ask === null) return false;
  return ask(messages.join("\n\n"));
}

function isMessage(value: string | null): value is string {
  return value !== null && value !== "";
}

function defaultConfirmer(): LeaveConfirmer | null {
  if (typeof window === "undefined" || typeof window.confirm !== "function") return null;
  return (message) => window.confirm(message);
}

/**
 * Warn before navigating away while `dirty` is true.
 *
 * The message is read when the guard is consulted, not when it was registered,
 * so it may describe the current state of the form.
 */
export function useUnsavedGuard(dirty: boolean, message: string): void {
  const messageRef = useRef(message);
  messageRef.current = message;

  useEffect(() => {
    if (!dirty) return;
    return registerGuard(() => messageRef.current);
  }, [dirty]);
}
