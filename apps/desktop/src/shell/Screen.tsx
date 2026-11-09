import { memo } from "react";

import { Activity } from "../routes/activity";
import { Agents } from "../routes/agents";
import { Approvals } from "../routes/approvals";
import { Dashboard } from "../routes/dashboard";
import type { Navigate, Route } from "../routes/route";
import { Schedules } from "../routes/schedules";
import { Settings } from "../routes/settings";
import { Tasks } from "../routes/tasks";

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
