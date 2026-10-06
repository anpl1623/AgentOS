/**
 * The router: the navigator in `history.ts`, bound to the window and to React.
 *
 * One navigator exists for the life of the window, made on first use, so the
 * route outlives any component and StrictMode's double mount cannot make two.
 * Screens still receive `navigate` as a prop, so a screen never imports the
 * router and could be rendered by anything that can produce a route.
 */

import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import type { Navigate, Route } from "../routes/route";
import { confirmLeave } from "../sdk/drafts";
import { type Navigator, type Place, createNavigator } from "./history";

let instance: Navigator | null = null;

function windowNavigator(): Navigator {
  instance ??= createNavigator({
    hash: () => window.location.hash,
    state: () => window.history.state as unknown,
    push: (state, hash) => window.history.pushState(state, "", hash),
    replace: (state, hash) => window.history.replaceState(state, "", hash),
    go: (delta) => window.history.go(delta),
    confirmLeave: () => confirmLeave(),
  });
  return instance;
}

/**
 * Follow the webview's history for as long as the app is mounted.
 *
 * `popstate` is the one signal for every move the platform makes — back,
 * forward, the mouse's buttons, and a hash typed by hand — so it is the only
 * listener; `hashchange` would report the same moves twice.
 */
export function RouterProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    const nav = windowNavigator();
    const onPop = () => nav.popped();
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  return <>{children}</>;
}

function subscribe(listener: () => void): () => void {
  return windowNavigator().subscribe(listener);
}

function place(): Place {
  return windowNavigator().place();
}

/** Where the window is, with its address and depth. */
export function usePlace(): Place {
  return useSyncExternalStore(subscribe, place);
}

/** The current route. */
export function useRoute(): Route {
  return usePlace().route;
}

/** Whether there is a screen of ours to go back to. */
export function useCanGoBack(): boolean {
  return usePlace().depth > 0;
}

const navigate: Navigate = (route) => {
  void windowNavigator().navigate(route);
};

const back = () => windowNavigator().back();
const forward = () => windowNavigator().forward();

/** Go to a route, asking first if a screen has unsaved work. Stable across renders. */
export function useNavigate(): Navigate {
  return navigate;
}

/** Go back one screen; does nothing at the first. Stable across renders. */
export function useBack(): () => void {
  return back;
}

/** Go forward one screen, if the history has one. Stable across renders. */
export function useForward(): () => void {
  return forward;
}
