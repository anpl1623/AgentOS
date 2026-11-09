/**
 * The colour theme: which one a person asked for, which one is showing, and
 * keeping the two in step.
 *
 * A person chooses `system` (the default), `dark` or `light`. The choice is
 * kept in localStorage under {@link THEME_STORAGE_KEY}; the theme it resolves
 * to is written to <html> as `data-theme`, which tokens.css reads, and as
 * `color-scheme`, which native controls and scrollbars read. main.tsx calls
 * {@link applyTheme} before React's first render, so the first frame React
 * paints is already in the right theme.
 *
 * Three things can change the theme afterwards, and all three go through one
 * store: a person choosing (here, through {@link useTheme} or
 * {@link setThemePreference}); the operating system switching between light
 * and dark while the choice is `system`; and another window of the application
 * changing the choice, which arrives as a `storage` event. A switch cross-fades
 * colours for a moment; loading never does, and reduced motion never does.
 *
 * The store is a module rather than a React context, like the alert store, so
 * the palette or a keyboard shortcut can change the theme without a provider.
 * The decisions are pure functions so they can be tested without a document.
 */

import { useSyncExternalStore } from "react";

/** What a person asked for. */
export type ThemePreference = "system" | "dark" | "light";

/** What is showing: a preference with `system` decided. */
export type ResolvedTheme = "dark" | "light";

/** Every preference, in the order a control offers them. */
export const THEME_PREFERENCES: readonly ThemePreference[] = ["system", "dark", "light"];

/** Where the preference is kept. */
export const THEME_STORAGE_KEY = "agentos.theme";

/**
 * How long colours cross-fade when the theme changes. The same figure as
 * `--dur-theme` in tokens.css, which the transition itself reads; this one
 * only decides when the transition class comes off again.
 */
export const THEME_CROSSFADE_MS = 300;

/** The class on <html> that turns the cross-fade on; see motion.css. */
export const THEME_SWITCHING_CLASS = "theme-switching";

const DARK_QUERY = "(prefers-color-scheme: dark)";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/** Whether `value` is one of the three preferences. */
export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (THEME_PREFERENCES as readonly string[]).includes(value);
}

/**
 * A stored value as a preference.
 *
 * Storage is written by this module but read back from a place anything can
 * write to, so a value this build does not know — a later version's, or a
 * hand-edited one — means `system` rather than a theme nobody chose.
 */
export function parseThemePreference(raw: string | null | undefined): ThemePreference {
  return isThemePreference(raw) ? raw : "system";
}

/** The theme a preference shows, given what the operating system prefers. */
export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  if (preference === "system") return systemPrefersDark ? "dark" : "light";
  return preference;
}

/** The preference after `preference`, for a control that cycles through them. */
export function nextThemePreference(preference: ThemePreference): ThemePreference {
  const at = THEME_PREFERENCES.indexOf(preference);
  return THEME_PREFERENCES[(at + 1) % THEME_PREFERENCES.length] ?? "system";
}

/**
 * The stored preference, or `system`.
 *
 * Reading storage can throw — a webview with storage disabled, a private
 * window, a quota error — and a theme is never worth failing to start over.
 */
export function readThemePreference(): ThemePreference {
  try {
    return parseThemePreference(globalThis.localStorage?.getItem(THEME_STORAGE_KEY));
  } catch {
    return "system";
  }
}

function writeThemePreference(preference: ThemePreference): void {
  try {
    globalThis.localStorage?.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // The choice still applies to this window; it just will not be remembered.
  }
}

function matches(query: string, fallback: boolean): boolean {
  try {
    return typeof globalThis.matchMedia === "function"
      ? globalThis.matchMedia(query).matches
      : fallback;
  } catch {
    return fallback;
  }
}

/** Dark is the application's home theme, so an unanswerable question means dark. */
function systemPrefersDark(): boolean {
  return matches(DARK_QUERY, true);
}

/** What the store holds, and what {@link useTheme} returns besides the setter. */
export interface ThemeState {
  preference: ThemePreference;
  resolved: ResolvedTheme;
}

let state: ThemeState = { preference: "system", resolved: "dark" };
let applied = false;
let fadeTimer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();

/** The current preference and theme, for code outside React. */
export function currentTheme(): ThemeState {
  return state;
}

/**
 * Write a theme onto the document.
 *
 * With `fade`, colours cross-fade: the class that declares the transition is
 * added in the same change as the new theme, and transitions take their
 * duration from the style after a change, so they run from the old colours to
 * the new ones. It comes off once they have finished, so nothing else in the
 * window ever transitions its colours by accident.
 */
function paint(resolved: ResolvedTheme, fade: boolean): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const changing = root.dataset.theme !== undefined && root.dataset.theme !== resolved;
  if (fade && changing && !matches(REDUCED_MOTION_QUERY, false)) {
    root.classList.add(THEME_SWITCHING_CLASS);
    clearTimeout(fadeTimer);
    // A little past the transition, so it is never cut off in its last frame.
    fadeTimer = setTimeout(() => {
      root.classList.remove(THEME_SWITCHING_CLASS);
    }, THEME_CROSSFADE_MS + 50);
  }
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
}

function publish(preference: ThemePreference, fade: boolean): void {
  const next: ThemeState = { preference, resolved: resolveTheme(preference, systemPrefersDark()) };
  paint(next.resolved, fade);
  if (next.preference === state.preference && next.resolved === state.resolved) return;
  state = next;
  for (const listener of listeners) listener();
}

/**
 * Follow the operating system and the other windows. Installed once, by the
 * first {@link applyTheme}, and for the life of the window.
 */
function install(): void {
  if (typeof globalThis.matchMedia === "function") {
    try {
      globalThis.matchMedia(DARK_QUERY).addEventListener("change", () => {
        if (state.preference === "system") publish("system", true);
      });
    } catch {
      // A webview that cannot report its scheme keeps whatever it started with.
    }
  }
  if (typeof globalThis.addEventListener === "function") {
    globalThis.addEventListener("storage", (event: StorageEvent) => {
      // A null key is storage being cleared, which takes the choice with it.
      if (event.key === THEME_STORAGE_KEY || event.key === null) {
        publish(readThemePreference(), true);
      }
    });
  }
}

/**
 * Read the stored preference and write the theme it resolves to onto <html>.
 *
 * Call once, before the first render. It never cross-fades: the window is
 * opening, not changing. Later calls re-read storage without reinstalling the
 * listeners.
 */
export function applyTheme(): void {
  publish(readThemePreference(), false);
  if (!applied) {
    applied = true;
    install();
  }
}

/** Choose a theme: remember it, show it, and tell every subscriber. */
export function setThemePreference(preference: ThemePreference): void {
  if (!isThemePreference(preference)) return;
  writeThemePreference(preference);
  publish(preference, true);
}

function subscribe(listener: () => void): () => void {
  if (!applied) applyTheme();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The theme for a component: what was asked for, what shows, and a way to change it. */
export function useTheme(): {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference(preference: ThemePreference): void;
} {
  const current = useSyncExternalStore(subscribe, currentTheme, currentTheme);
  return {
    preference: current.preference,
    resolved: current.resolved,
    setPreference: setThemePreference,
  };
}
