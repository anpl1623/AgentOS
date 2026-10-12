import { useCallback, useEffect, useRef, useState } from "react";

import { describeError } from "./client";

export { useRefresh } from "./live";

/** The state of something being loaded from the runtime. */
export interface Async<T> {
  /** The last value that loaded successfully, kept through later failures. */
  data: T | null;
  /** Why the most recent load failed, cleared by the next success. */
  error: string | null;
  /**
   * A load is in flight and there is nothing to show yet.
   *
   * Never true while `data` is present, so a screen that renders a spinner on
   * `loading` does not blank itself every time a poll comes round.
   */
  loading: boolean;
  /** A load is in flight and `data` is on screen meanwhile. */
  refreshing: boolean;
  /** The most recent load failed, so `data` is older than the last attempt. */
  stale: boolean;
  /** Re-run the loader. Safe to call from an event handler. */
  reload: () => void;
}

/** Hooks for layering behaviour over {@link useAsync} without copying it. */
export interface AsyncOptions<T> {
  /**
   * The value to show before the first load of a scope settles, read on mount
   * and again whenever the dependencies change.
   */
  seed?: () => T | null;
  /** Called with every successful result that is accepted. */
  onSuccess?: (value: T) => void;
}

/** Everything {@link useAsync} remembers between renders, apart from the loader. */
export interface AsyncState<T> {
  data: T | null;
  error: string | null;
  inFlight: boolean;
  /** Which load the state is waiting on; an answer from any other is stale. */
  generation: number;
}

/** What can happen to an {@link AsyncState}. */
export type AsyncEvent<T> =
  /** A load was issued. */
  | { kind: "start"; generation: number }
  /** A load answered. */
  | { kind: "success"; generation: number; value: T }
  /** A load failed. */
  | { kind: "failure"; generation: number; error: string }
  /** The dependencies changed: a different question, starting from `seed`. */
  | { kind: "scope"; seed: T | null };

/** The state before anything has loaded. */
export function initialAsync<T>(seed: T | null): AsyncState<T> {
  return { data: seed, error: null, inFlight: true, generation: 0 };
}

/**
 * The one place the rules of {@link useAsync} live, as a pure function so each
 * can be tested without a renderer.
 *
 * - A success replaces the data and clears the error.
 * - A failure keeps the data and records the error, so the data becomes stale.
 * - An answer from any load but the latest issued is ignored: a slow first
 *   request must not overwrite a fast second one.
 * - A change of scope drops the data, because it answered another question.
 */
export function stepAsync<T>(state: AsyncState<T>, event: AsyncEvent<T>): AsyncState<T> {
  switch (event.kind) {
    case "start":
      return { ...state, inFlight: true, generation: event.generation };
    case "success":
      if (event.generation !== state.generation) return state;
      return { data: event.value, error: null, inFlight: false, generation: state.generation };
    case "failure":
      if (event.generation !== state.generation) return state;
      return { ...state, error: event.error, inFlight: false };
    case "scope":
      return { data: event.seed, error: null, inFlight: true, generation: state.generation };
  }
}

/** What a screen reads from an {@link AsyncState}. */
export function viewAsync<T>(state: AsyncState<T>): Omit<Async<T>, "reload"> {
  const hasData = state.data !== null;
  return {
    data: state.data,
    error: state.error,
    loading: state.inFlight && !hasData,
    refreshing: state.inFlight && hasData,
    stale: state.error !== null && hasData,
  };
}

/**
 * Load something from the runtime, with reloading and an error surface.
 *
 * Screens use this rather than each hand-rolling loading and error state, which
 * is how one screen ends up silently swallowing a failure that another reports.
 *
 * A reload that arrives after the component unmounts, or after a newer reload
 * has started, is discarded — otherwise a slow first request can overwrite the
 * result of a fast second one. Discarding is all that can be done: a Tauri
 * `invoke` cannot be cancelled once issued, so the only real saving is not
 * issuing it, which is what `useVisibleInterval` is for.
 *
 * A failed reload keeps the data it had and marks it stale rather than
 * blanking the screen; a person looking at a dashboard is better served by the
 * last known state with a warning than by an error where the state was.
 *
 * Changing the dependencies starts a new scope: the previous scope's data
 * belongs to a different question, so it is dropped rather than shown as a
 * refresh of the new one.
 */
export function useAsync<T>(
  load: () => Promise<T>,
  deps: readonly unknown[] = [],
  options: AsyncOptions<T> = {},
): Async<T> {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const [state, setState] = useState<AsyncState<T>>(() =>
    initialAsync(options.seed?.() ?? null),
  );
  const [scope, setScope] = useState(deps);
  if (!sameDeps(scope, deps)) {
    // Adjusting state during render, rather than in an effect, so the first
    // paint of the new scope never shows the old scope's data.
    setScope(deps);
    setState((previous) =>
      stepAsync(previous, { kind: "scope", seed: optionsRef.current.seed?.() ?? null }),
    );
  }

  const [nonce, setNonce] = useState(0);
  const generation = useRef(0);
  const loadRef = useRef(load);
  loadRef.current = load;

  useEffect(() => {
    const current = ++generation.current;
    let cancelled = false;
    setState((previous) => stepAsync(previous, { kind: "start", generation: current }));

    loadRef
      .current()
      .then((value) => {
        if (cancelled || current !== generation.current) return;
        setState((previous) =>
          stepAsync(previous, { kind: "success", generation: current, value }),
        );
        optionsRef.current.onSuccess?.(value);
      })
      .catch((failure: unknown) => {
        if (cancelled || current !== generation.current) return;
        setState((previous) =>
          stepAsync(previous, {
            kind: "failure",
            generation: current,
            error: describeError(failure),
          }),
        );
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nonce, ...deps]);

  const reload = useCallback(() => setNonce((value) => value + 1), []);
  return { ...viewAsync(state), reload };
}

function sameDeps(a: readonly unknown[], b: readonly unknown[]): boolean {
  return a.length === b.length && a.every((value, index) => Object.is(value, b[index]));
}
