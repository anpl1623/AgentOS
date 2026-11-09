import { useEffect, useState } from "react";

import type { CadenceInput } from "../../bindings/CadenceInput";
import type { CadencePreview } from "../../bindings/CadencePreview";
import { api } from "../../sdk/client";
import { useAsync } from "../../sdk/useAsync";

/**
 * How long typing must pause before the cadence is checked.
 *
 * Long enough that `0 9 * * MON-FRI` typed at speed is one request rather than
 * fifteen, short enough that the answer arrives while the operator is still
 * looking at the field.
 */
const CHECK_DEBOUNCE_MS = 250;

/**
 * The runtime's check of the cadence as typed, settled after a pause in
 * typing.
 *
 * `preview` and `error` are only ever the answer for exactly `input`: while a
 * newer keystroke is waiting or in flight both are `null`, so an answer for
 * `0 9 * *` is never shown beneath `0 9 * * 1-5`.
 */
export function useCadenceCheck(input: CadenceInput | null): {
  preview: CadencePreview | null;
  error: string | null;
} {
  const key = input === null ? null : JSON.stringify(input);
  const [settled, setSettled] = useState(key);
  useEffect(() => {
    if (key === settled) return;
    const timer = window.setTimeout(() => setSettled(key), CHECK_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [key, settled]);

  const check = useAsync<CadencePreview | null>(
    () =>
      settled === null
        ? Promise.resolve(null)
        : api.checkCadence(JSON.parse(settled) as CadenceInput),
    [settled],
  );
  const current = key === settled && !check.loading && !check.refreshing;
  return { preview: current ? check.data : null, error: current ? check.error : null };
}
