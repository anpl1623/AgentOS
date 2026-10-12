/**
 * The screens, in the order the sidebar lists them.
 *
 * One list serves the sidebar, the `Cmd/Ctrl+1..7` accelerators, the palette
 * and the window title, so the number a screen answers to is always its place
 * in the sidebar.
 */

import type { Route, RouteName } from "../routes/route";

/** A screen as the chrome names it. */
export interface NavItem {
  route: RouteName;
  label: string;
  /** What the screen is for, in a few words; the palette searches it. */
  description: string;
}

export const NAV: readonly NavItem[] = [
  { route: "dashboard", label: "Dashboard", description: "What is running and what needs you" },
  { route: "approvals", label: "Approvals", description: "Actions waiting on your decision" },
  { route: "tasks", label: "Tasks", description: "Objectives, runs and their traces" },
  { route: "schedules", label: "Schedules", description: "Standing instructions on a timer" },
  { route: "agents", label: "Agents", description: "Agents, their tools and policies" },
  { route: "activity", label: "Activity", description: "Everything the runtime has recorded" },
  { route: "settings", label: "Settings", description: "Providers, tools and the audit log" },
];

/** The sidebar label of a screen. */
export function labelFor(name: RouteName): string {
  return NAV.find((item) => item.route === name)?.label ?? "AgentOS";
}

/** The window title for a route. */
export function titleFor(route: Route): string {
  return `${labelFor(route.name)} — AgentOS`;
}

/**
 * The route for a screen with no parameters. Every parameter in the union is
 * optional, so a bare name is always a whole route; the cast says so once.
 */
export function screenRoute(name: RouteName): Route {
  return { name } as Route;
}
