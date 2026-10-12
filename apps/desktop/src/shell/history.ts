/**
 * Moving between screens through the webview's own history.
 *
 * Every screen change is a history entry, so back and forward — the mouse's
 * buttons included — work with no code of ours in the way, and a reload opens
 * the screen that was showing. Each entry we write carries its depth, a count
 * of our entries behind it, so "can I go back?" has an honest answer after a
 * reload rather than a guess, and back is never allowed to leave the app.
 *
 * Leaving a screen with unsaved work asks first, through the guard registry in
 * `sdk/drafts.ts`. A forward navigation asks before it moves. Back and forward
 * cannot: the platform has already moved when it says so. They ask afterwards
 * and, on a refusal, step the history back to where it was.
 *
 * The logic is written against a small host interface rather than `window`, so
 * the tests can drive it with a history they control.
 */

import type { Route } from "../routes/route";
import { fromHash, toHash } from "./hash";

/** Where the window is, and how many of our entries lie behind it. */
export interface Place {
  route: Route;
  /** The canonical address of `route`. */
  hash: string;
  depth: number;
}

/** What the navigator needs from the browser. */
export interface HistoryHost {
  /** The current `location.hash`. */
  hash: () => string;
  /** The current `history.state`. */
  state: () => unknown;
  push: (state: unknown, hash: string) => void;
  replace: (state: unknown, hash: string) => void;
  go: (delta: number) => void;
  /** Whether it is all right to leave the current screen. */
  confirmLeave: () => Promise<boolean>;
}

/** The window's position in its history, and the ways to change it. */
export interface Navigator {
  place: () => Place;
  subscribe: (listener: () => void) => () => void;
  /** Go to a route. Resolves whether the window moved. */
  navigate: (route: Route) => Promise<boolean>;
  back: () => void;
  forward: () => void;
  /** Tell the navigator the platform moved through history: a `popstate`. */
  popped: () => void;
}

const DEPTH_KEY = "agentosDepth";

/** The history state written for an entry at `depth`. */
export function stateFor(depth: number): Record<string, number> {
  return { [DEPTH_KEY]: depth };
}

/** The depth an entry records, or `null` for an entry this app did not write. */
export function readDepth(state: unknown): number | null {
  if (typeof state !== "object" || state === null || !(DEPTH_KEY in state)) return null;
  const depth = (state as Record<string, unknown>)[DEPTH_KEY];
  return typeof depth === "number" && Number.isSafeInteger(depth) && depth >= 0 ? depth : null;
}

/** The place a hash names, at `depth`. */
function placeAt(hash: string, depth: number): Place {
  const route = fromHash(hash);
  return { route, hash: toHash(route), depth };
}

/**
 * Start navigating from wherever the host's history is.
 *
 * The current entry is rewritten to its canonical address and given a depth if
 * it has none, so a malformed link shows the dashboard's address rather than
 * the link, and the first screen is an entry like every other.
 */
export function createNavigator(host: HistoryHost): Navigator {
  let current = placeAt(host.hash(), readDepth(host.state()) ?? 0);
  host.replace(stateFor(current.depth), current.hash);

  const listeners = new Set<() => void>();
  // A question is open. Further moves wait for its answer rather than stack
  // a second question on top of it.
  let asking = false;
  // The pop the navigator itself caused by stepping back after a refusal.
  let undoing = false;

  function commit(next: Place): void {
    if (next.hash === current.hash && next.depth === current.depth) return;
    current = next;
    for (const listener of listeners) listener();
  }

  async function ask(): Promise<boolean> {
    asking = true;
    try {
      return await host.confirmLeave();
    } catch (failure) {
      console.error("A navigation guard failed; staying on this screen", failure);
      return false;
    } finally {
      asking = false;
    }
  }

  /** Whether the history has moved away from `from` since it was current. */
  function movedFrom(from: Place): boolean {
    return host.hash() !== from.hash || readDepth(host.state()) !== from.depth;
  }

  /** Put the history back on `from`, after a refusal to leave it. */
  function restore(from: Place): void {
    const depth = readDepth(host.state());
    if (depth !== null && depth !== from.depth) {
      undoing = true;
      host.go(from.depth - depth);
    } else {
      // An entry with no depth was not written here — a hash typed into the
      // address bar — so there is nowhere known to step to. It becomes the
      // screen the person chose to stay on.
      host.replace(stateFor(from.depth), from.hash);
    }
  }

  async function navigate(route: Route): Promise<boolean> {
    const hash = toHash(route);
    if (hash === current.hash || asking) return false;
    const from = current;
    const leave = await ask();
    // The platform does not wait for the answer: a mouse's back button can
    // move the history while the question is open, and the pop it sends is
    // left to this answer. So the answer is applied to wherever the history
    // is now, as `popped` applies its own, and the window, the address and
    // the recorded depth agree again whichever way the person answered.
    const moved = movedFrom(from);
    if (!leave) {
      if (moved) restore(from);
      return false;
    }
    let depth = from.depth;
    if (moved) {
      const present = readDepth(host.state());
      depth = present ?? from.depth + 1;
      // An entry typed while the question was open is given its depth, as
      // `popped` would have, so stepping back onto it later reads true.
      if (present === null) host.replace(stateFor(depth), placeAt(host.hash(), depth).hash);
    }
    const next = placeAt(hash, depth + 1);
    host.push(stateFor(next.depth), next.hash);
    commit(next);
    return true;
  }

  function popped(): void {
    if (undoing) {
      undoing = false;
      if (placeAt(host.hash(), 0).hash === current.hash) return;
    }
    // A move while a question is open is settled when the question is: both
    // `navigate` and this re-read the history once the person has answered.
    if (asking) return;

    const from = current;
    void ask().then((leave) => {
      // Read the position now, not when the pop arrived: it is what the
      // history holds once the person has answered.
      if (leave) {
        const depth = readDepth(host.state());
        const next = placeAt(host.hash(), depth ?? from.depth + 1);
        if (depth === null || host.hash() !== next.hash) {
          host.replace(stateFor(next.depth), next.hash);
        }
        commit(next);
        return;
      }
      restore(from);
    });
  }

  return {
    place: () => current,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    navigate,
    back() {
      if (current.depth > 0 && !asking) host.go(-1);
    },
    forward() {
      if (!asking) host.go(1);
    },
    popped,
  };
}
