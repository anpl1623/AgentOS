/**
 * Where the window is.
 *
 * A discriminated union rather than a routing library: seven screens, four of
 * which take an optional identifier, does not justify a dependency — and this
 * way an impossible route (a task screen with an agent name) does not typecheck.
 *
 * The union is frozen for the screens still to come. A screen that needs a new
 * parameter is a change here, in the hash codec beside the shell, and in its
 * round-trip test, so an address can never name a place the window cannot
 * reopen.
 */
export type Route =
  | { name: "dashboard" }
  /** `focus` is one pending approval, so a link can send an operator to the exact card. */
  | { name: "approvals"; focus?: string }
  | { name: "tasks"; runId?: string }
  | { name: "agents"; agent?: string }
  /** `runId` narrows the feed to one run. */
  | { name: "activity"; runId?: string }
  | { name: "schedules" }
  | { name: "settings" };

/** The name of a screen, without its parameters. */
export type RouteName = Route["name"];

export type Navigate = (route: Route) => void;
