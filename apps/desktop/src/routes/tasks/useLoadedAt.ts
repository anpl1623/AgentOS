import { useEffect, useState } from "react";

/**
 * When `data` last changed to something, as the moment a load succeeded.
 *
 * `useAsync` keeps the last good answer through a failed reload; this is how
 * old that answer is, for the `Stale` note beside it. Shared by the task list
 * and the run, which both say how old what they show is.
 */
export function useLoadedAt(data: unknown): number | null {
  const [at, setAt] = useState<number | null>(null);
  useEffect(() => {
    if (data !== null) setAt(Date.now());
  }, [data]);
  return at;
}
