import { useCallback, useEffect, useState } from "react";

/**
 * Silence a live region through deliberate changes to what it holds.
 *
 * `hush` before the change, `release` once it has been made. The region
 * speaks again one commit after the last release, never in the commit that
 * changed its rows, so what a release lets through is only what arrives
 * afterwards. Counted, so one change finishing does not unsilence another
 * still in progress.
 */
export function useHush() {
  const [pending, setPending] = useState(0);
  const [finished, setFinished] = useState(0);
  useEffect(() => {
    if (finished === 0) return;
    setPending((count) => Math.max(0, count - finished));
    setFinished(0);
  }, [finished]);
  const hush = useCallback(() => setPending((count) => count + 1), []);
  const release = useCallback(() => setFinished((count) => count + 1), []);
  return { hushed: pending > 0, hush, release };
}
