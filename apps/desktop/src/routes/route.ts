/**
 * Where the window is.
 *
 * A discriminated union rather than a routing library: six screens, two of which
 * take an optional identifier, does not justify a dependency — and this way an
 * impossible route (a task screen with an agent name) does not typecheck.
 */
export type Route =
  | { name: "dashboard" }
  | { name: "approvals" }
  | { name: "tasks"; runId?: string }
  | { name: "agents"; agent?: string }
  | { name: "activity" }
  | { name: "settings" };

export type Navigate = (route: Route) => void;
