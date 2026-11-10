/**
 * The layout decisions in `layout.ts`, bound to the window.
 */

import { useCallback, useState, useSyncExternalStore } from "react";

import {
  BREAKPOINTS,
  type LayoutMode,
  SIDEBAR_KEY,
  layoutMode,
  parseSidebarPreference,
  serialiseSidebarPreference,
} from "./layout";

/**
 * Re-read on the media queries' own change events rather than every resize:
 * the mode moves at two widths, and a drag across the rest of the range has
 * nothing to tell React.
 */
function subscribe(listener: () => void): () => void {
  const queries = [BREAKPOINTS.compact, BREAKPOINTS.wide].map((px) =>
    window.matchMedia(`(min-width: ${px}px)`),
  );
  for (const query of queries) query.addEventListener("change", listener);
  return () => {
    for (const query of queries) query.removeEventListener("change", listener);
  };
}

function snapshot(): LayoutMode {
  return layoutMode(window.innerWidth);
}

/**
 * The window's layout mode, read synchronously on the first render, so the
 * sidebar's button is labelled for the layout the stylesheet is drawing from
 * the first frame.
 */
export function useLayoutMode(): LayoutMode {
  return useSyncExternalStore(subscribe, snapshot);
}

function readCollapsed(): boolean {
  try {
    return parseSidebarPreference(window.localStorage.getItem(SIDEBAR_KEY));
  } catch {
    // Storage can be switched off or full; the full sidebar is the default
    // that hides nothing.
    return false;
  }
}

function writeCollapsed(collapsed: boolean): void {
  try {
    window.localStorage.setItem(SIDEBAR_KEY, serialiseSidebarPreference(collapsed));
  } catch {
    // Not remembered across launches; this window still honours it.
  }
}

/**
 * Whether the operator has collapsed the wide sidebar to its rail, kept
 * between launches. Read before the first paint, so a collapsed sidebar does
 * not open full and then shut.
 */
export function useSidebarCollapsed(): [boolean, (collapsed: boolean) => void] {
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const set = useCallback((next: boolean) => {
    setCollapsed(next);
    writeCollapsed(next);
  }, []);
  return [collapsed, set];
}
