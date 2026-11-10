import { memo, use } from "react";

import type { Navigate, Route } from "../routes/route";
import { createModuleCache } from "./moduleCache";

/**
 * Each screen's module, fetched on first use.
 *
 * A screen is most of the code in the window, and a person opens one at a
 * time, so each is its own chunk: the first paint waits only for the shell
 * and the screen it lands on. The rest are fetched once the window is idle
 * (`preloadScreens`), so moving to a screen later rarely waits at all.
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

/**
 * The screens' modules, one promise each for the life of the window, filled
 * by the preload and read by the draw with `use` rather than through `lazy`.
 * `lazy` fetches through a promise of its own however recently the preload
 * fetched the same module, so a first visit still suspended, committed the
 * skeleton and waited out React's fallback throttle: some 300ms of blank
 * column on a move that had nothing to wait for. The cache marks a module
 * that has arrived, so it is drawn in the same render as the move, and only
 * a screen whose code is really still on its way shows the skeleton.
 */
const screens = createModuleCache(LOADERS);

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
    for (const name of Object.keys(LOADERS) as Route["name"][]) {
      void screens.preload(name);
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
    case "dashboard": {
      const { Dashboard } = use(screens.read("dashboard"));
      return <Dashboard navigate={navigate} />;
    }
    case "approvals": {
      const { Approvals } = use(screens.read("approvals"));
      return <Approvals focus={route.focus} />;
    }
    case "tasks": {
      const { Tasks } = use(screens.read("tasks"));
      return <Tasks route={route} navigate={navigate} />;
    }
    case "schedules": {
      const { Schedules } = use(screens.read("schedules"));
      return <Schedules />;
    }
    case "agents": {
      const { Agents } = use(screens.read("agents"));
      return <Agents route={route} navigate={navigate} />;
    }
    case "activity": {
      const { Activity } = use(screens.read("activity"));
      return <Activity runId={route.runId} navigate={navigate} />;
    }
    case "settings": {
      const { Settings } = use(screens.read("settings"));
      return <Settings navigate={navigate} />;
    }
  }
});
