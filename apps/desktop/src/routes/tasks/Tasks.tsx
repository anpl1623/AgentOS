import { useState } from "react";

import type { Navigate, Route } from "../route";
import { TaskList } from "./list/TaskList";
import { Run } from "./run/Run";

/**
 * Tasks: the work given to agents, and one run of it.
 *
 * Two screens behind one route. The list (`list/`) is where work is given and
 * found; the run (`run/`) is the page an operator opens after a failure, and
 * it is read differently from the list.
 *
 * The shell raises a screen into place when the route's name changes, and
 * only then, so a move between the list and a run is this screen's own to
 * animate. `entering` is false on the first draw, when the shell's entrance
 * is already playing, and true for every view that replaces another.
 */
export function Tasks({
  route,
  navigate,
}: {
  route: Route & { name: "tasks" };
  navigate: Navigate;
}) {
  // The view first drawn, and whether the screen has since moved off it: the
  // usual adjustment of state during render, which React allows for state
  // derived from props.
  const view = route.runId ?? null;
  const [first] = useState(view);
  const [moved, setMoved] = useState(false);
  if (!moved && view !== first) setMoved(true);
  const entering = moved || view !== first;
  if (route.runId) {
    // Keyed so a different run starts from nothing: a stop requested on one
    // attempt must not show as `cancelling` on the next.
    return <Run key={route.runId} runId={route.runId} navigate={navigate} entering={entering} />;
  }
  return <TaskList navigate={navigate} entering={entering} />;
}
