/**
 * The address of every screen, as the part of the URL after `#`.
 *
 * A route lives in the hash so the window can be reopened where it was left,
 * a link can name one approval or one run, and the webview's own history — the
 * mouse's back button included — moves between screens with no code of ours
 * in the way. The codec is pure and total: every route has one address, and
 * every string is some route, because a malformed hash is a bad link and not a
 * reason for the window to fail.
 *
 * Identifiers are encoded with `encodeURIComponent`, which escapes `/`, `#`,
 * `%`, `?` and spaces, so a path segment can be split on a raw `/` and an agent
 * may be named anything at all. An empty identifier is no identifier: it has
 * no address of its own and reads back as the screen without one.
 */

import type { Route } from "../routes/route";

/** The address of the dashboard, which is also where anything unreadable lands. */
export const DASHBOARD_HASH = "#/dashboard";

function segment(value: string): string {
  return encodeURIComponent(value);
}

/** The address of a route, beginning with `#/`. */
export function toHash(route: Route): string {
  switch (route.name) {
    case "dashboard":
      return DASHBOARD_HASH;
    case "approvals":
      return route.focus ? `#/approvals/${segment(route.focus)}` : "#/approvals";
    case "tasks":
      return route.runId ? `#/tasks/run/${segment(route.runId)}` : "#/tasks";
    case "agents":
      return route.agent ? `#/agents/${segment(route.agent)}` : "#/agents";
    case "activity":
      return route.runId ? `#/activity/run/${segment(route.runId)}` : "#/activity";
    case "schedules":
      return "#/schedules";
    case "settings":
      return "#/settings";
  }
}

/**
 * The route an address names, or the dashboard when it names none.
 *
 * Accepts the hash with or without its leading `#`. Only the exact shapes
 * {@link toHash} writes are read; anything else — an unknown screen, a segment
 * too many, an escape that does not decode — is the dashboard, never an error.
 */
export function fromHash(hash: string): Route {
  const dashboard: Route = { name: "dashboard" };
  const path = hash.startsWith("#") ? hash.slice(1) : hash;
  if (path === "" || path === "/") return dashboard;
  if (!path.startsWith("/")) return dashboard;

  let parts: string[];
  try {
    parts = path.slice(1).split("/").map(decodeURIComponent);
  } catch {
    // A `%` that does not begin a valid escape.
    return dashboard;
  }

  const [screen, first, second, ...rest] = parts;
  if (rest.length > 0) return dashboard;

  switch (screen) {
    case "dashboard":
    case "schedules":
    case "settings":
      return first === undefined ? { name: screen } : dashboard;
    case "approvals":
      if (first === undefined) return { name: "approvals" };
      return first !== "" && second === undefined ? { name: "approvals", focus: first } : dashboard;
    case "agents":
      if (first === undefined) return { name: "agents" };
      return first !== "" && second === undefined ? { name: "agents", agent: first } : dashboard;
    case "tasks":
    case "activity":
      if (first === undefined) return { name: screen };
      return first === "run" && second !== undefined && second !== ""
        ? { name: screen, runId: second }
        : dashboard;
    default:
      return dashboard;
  }
}
