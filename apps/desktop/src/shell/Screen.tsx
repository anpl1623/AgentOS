import { lazy, memo } from "react";

import type { Navigate, Route } from "../routes/route";

/**
 * Each screen's module, fetched on first use.
 *
 * A screen is most of the code in the window, and a person opens one at a
 * time, so each is its own chunk: the first paint waits only for the shell
 * and the screen it lands on. The rest are fetched once the window is idle
 * (`preloadScreens`), so moving to a screen later rarely waits at all.
 *
 * The browser keeps a module once it has fetched it, so a later import of the
 * same one — from a move, or from the preload — resolves with that module and
 * fetches nothing.
 */
const LOADERS = {
  dashboard: () => import("../routes/dashboard"),
  approvals: () => import("../routes/approvals"),
  tasks: () => import("../routes/tasks"),
  schedules: () => import("../routes/schedules"),
  agents: () => import("../routes/agents"),
  activity: () => import("../routes/activity"),
  settings: () => import("../routes/settings"),
} satisfies Record<Route["name"], () => Promise<unknown>>;

// Each screen's named export, as the default export `lazy` expects. Written
// out one by one so every screen keeps its own props' types.
const Dashboard = lazy(() => LOADERS.dashboard().then((m) => ({ default: m.Dashboard })));
const Approvals = lazy(() => LOADERS.approvals().then((m) => ({ default: m.Approvals })));
const Tasks = lazy(() => LOADERS.tasks().then((m) => ({ default: m.Tasks })));
const Schedules = lazy(() => LOADERS.schedules().then((m) => ({ default: m.Schedules })));
const Agents = lazy(() => LOADERS.agents().then((m) => ({ default: m.Agents })));
const Activity = lazy(() => LOADERS.activity().then((m) => ({ default: m.Activity })));
const Settings = lazy(() => LOADERS.settings().then((m) => ({ default: m.Settings })));

/** How long after the first screen is drawn the rest are fetched, where idle callbacks are missing. */
const PRELOAD_FALLBACK_MS = 1_500;

/**
 * Fetch every screen's module without drawing it, once the window is idle,
 * so a later move finds it ready. Returns a function that cancels the fetch
 * if it has not started. A failure is left for the move itself to meet,
 * where the screen's error boundary says so; here it would have no one to
 * tell.
 */
export function preloadScreens(): () => void {
  const fetchAll = () => {
    for (const load of Object.values(LOADERS)) {
      load().catch(() => undefined);
    }
  };
  // WebKit, which the window runs on macOS, has no idle callback; a short
  // timer after the first screen serves the same purpose there.
  if (typeof window.requestIdleCallback === "function") {
    const handle = window.requestIdleCallback(fetchAll, { timeout: PRELOAD_FALLBACK_MS * 2 });
    return () => window.cancelIdleCallback(handle);
  }
  const timer = window.setTimeout(fetchAll, PRELOAD_FALLBACK_MS);
  return () => window.clearTimeout(timer);
}

/** The screen for a route, with the props its route implies. */
export const Screen = memo(function Screen({
  route,
  navigate,
}: {
  route: Route;
  navigate: Navigate;
}) {
  switch (route.name) {
    case "dashboard":
      return <Dashboard navigate={navigate} />;
    case "approvals":
      return <Approvals focus={route.focus} />;
    case "tasks":
      return <Tasks route={route} navigate={navigate} />;
    case "schedules":
      return <Schedules />;
    case "agents":
      return <Agents route={route} navigate={navigate} />;
    case "activity":
      return <Activity runId={route.runId} navigate={navigate} />;
    case "settings":
      return <Settings navigate={navigate} />;
  }
});
