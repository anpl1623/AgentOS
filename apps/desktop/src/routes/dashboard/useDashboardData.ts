import { useEffect, useRef, useState } from "react";

import { usePendingApprovals } from "../../sdk/approvals";
import { cacheKey, useAsyncCached } from "../../sdk/cache";
import { api } from "../../sdk/client";
import { useVisibleInterval } from "../../sdk/live";
import { useAsync } from "../../sdk/useAsync";
import { type ToolWindow, refusalRows } from "./dashboardModel";
import { readiness } from "./readinessModel";

/**
 * How often the dashboard itself is re-read while visible.
 *
 * Slower than the 4s default because nothing on it waits on the poll to be
 * current: the screen also refreshes whenever it becomes visible, and the
 * Needs-you panel reads the approval store, which learns of requests by event.
 */
const DASHBOARD_MS = 8_000;

/**
 * How often the slower reads are repeated while visible: the audit chain's
 * health, tool usage, and setup. The chain is checked incrementally, so each
 * check costs only the records written since the last; usage is an aggregate
 * that a few seconds' staleness does not change the reading of.
 */
const SLOW_MS = 60_000;

/**
 * Every read the dashboard makes, on its own schedule, in one place.
 *
 * The components draw; this decides what is asked of the runtime and how
 * often. The order of the hooks is the order the screen has always made them
 * in, and each cache key is the one other screens share, so a revisit shows
 * what was last read at once.
 */
export function useDashboardData() {
  const view = useAsyncCached(cacheKey("dashboard"), () => api.dashboard());
  useVisibleInterval(view.reload, DASHBOARD_MS);
  const pending = usePendingApprovals();

  // The chain's health on its own, slower schedule: it changes only when
  // records are written, and the dashboard's poll should not pay for it. Not
  // cached: an answer carried over from an earlier visit would be shown as
  // this visit's check.
  const health = useAsync(() => api.auditHealth(), []);
  useVisibleInterval(health.reload, SLOW_MS);

  const [days, setDays] = useState<ToolWindow>(7);
  const usage = useAsyncCached(cacheKey("tool_usage", days), () => api.toolUsage(days), [days]);
  const today = useAsyncCached(cacheKey("tool_usage", 1), () => api.toolUsage(1));
  useVisibleInterval(() => {
    usage.reload();
    today.reload();
  }, SLOW_MS);

  const data = view.data;
  const refusals = data ? refusalRows(data.recent_refusals) : [];

  // A new refusal arriving on the fast poll moves the day's count at once
  // rather than up to a minute later, beside a list that already shows it.
  // The first refusal seen is the one the day's count was just read with.
  const newestRefusal = refusals[0]?.id;
  const seenRefusal = useRef(newestRefusal);
  const reloadToday = today.reload;
  useEffect(() => {
    if (newestRefusal === undefined || newestRefusal === seenRefusal.current) return;
    if (seenRefusal.current !== undefined) reloadToday();
    seenRefusal.current = newestRefusal;
  }, [newestRefusal, reloadToday]);

  const settings = useAsyncCached(cacheKey("settings"), () => api.settings());
  const missing = readiness(settings.data, data?.agents ?? null);
  // Setup is re-read only while something is missing; once it is complete
  // there is nothing on this screen for a re-read to change.
  useVisibleInterval(() => {
    if (missing.length > 0) settings.reload();
  }, SLOW_MS);

  // Placeholders only before the first answer. A failed first read leaves the
  // panels empty beneath its error rather than shimmering forever.
  const first = data === null && view.loading;

  return {
    view,
    data,
    first,
    pending,
    health,
    days,
    setDays,
    usage,
    today,
    refusals,
    settings,
    missing,
  };
}
