import { useCallback, useEffect, useRef, useState } from "react";

import { describeError } from "./client";

/** The state of something being loaded from the runtime. */
export interface Async<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  /** Re-run the loader. Safe to call from an event handler. */
  reload: () => void;
}

/**
 * Load something from the runtime, with reloading and an error surface.
 *
 * Screens use this rather than each hand-rolling loading and error state, which
 * is how one screen ends up silently swallowing a failure that another reports.
 *
 * A reload that arrives after the component unmounts, or after a newer reload
 * has started, is discarded — otherwise a slow first request can overwrite the
 * result of a fast second one.
 */
export function useAsync<T>(load: () => Promise<T>, deps: readonly unknown[] = []): Async<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);

  const generation = useRef(0);
  const loadRef = useRef(load);
  loadRef.current = load;

  useEffect(() => {
    const current = ++generation.current;
    let cancelled = false;
    setLoading(true);

    loadRef
      .current()
      .then((value) => {
        if (cancelled || current !== generation.current) return;
        setData(value);
        setError(null);
      })
      .catch((failure: unknown) => {
        if (cancelled || current !== generation.current) return;
        setError(describeError(failure));
      })
      .finally(() => {
        if (cancelled || current !== generation.current) return;
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nonce, ...deps]);

  const reload = useCallback(() => setNonce((value) => value + 1), []);
  return { data, error, loading, reload };
}

/**
 * Re-run something on an interval, and immediately when the window regains focus.
 *
 * Live runs change without the interface asking, and the activity stream only
 * carries events — not the derived state a screen shows. Polling keeps the two
 * from drifting without every screen inventing its own refresh.
 */
export function useRefresh(reload: () => void, everyMs = 4000): void {
  useEffect(() => {
    const timer = setInterval(reload, everyMs);
    const onFocus = () => reload();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [reload, everyMs]);
}
