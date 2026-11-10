/**
 * How the shell lays itself out, as pure decisions.
 *
 * The geometry itself is CSS: layout.css switches between the three modes
 * with media queries, so the first paint is already right and a resize never
 * waits on a render. What JavaScript holds is only what a stylesheet cannot —
 * whether the drawer is open, and whether the operator collapsed the sidebar —
 * and what the one sidebar button should say and do in each mode. Those
 * decisions live here, as functions of their inputs, so every combination is
 * tested without a window.
 */

/**
 * The window's three shapes.
 *
 * - `wide`: the full sidebar, with icons, labels and counts. It can be
 *   collapsed to the rail, and stays that way across launches.
 * - `compact`: the icon rail, its labels shown as tooltips. The rail's button
 *   opens the full sidebar as a drawer over the screen.
 * - `narrow`: no sidebar column at all. A top bar carries the menu, the
 *   screen's name, the palette and the approvals count, and the menu opens
 *   the sidebar as a drawer from the left.
 */
export type LayoutMode = "wide" | "compact" | "narrow";

/**
 * Where each mode begins, in CSS pixels. The window's own minimum is 940 by
 * 620, so `narrow` is reached by zooming: 200% puts that window at 470.
 */
export const BREAKPOINTS = { compact: 720, wide: 1200 } as const;

/** The mode for a viewport `width` wide. */
export function layoutMode(width: number): LayoutMode {
  if (width >= BREAKPOINTS.wide) return "wide";
  if (width >= BREAKPOINTS.compact) return "compact";
  // A width that cannot be read (NaN) lands here too: the narrow layout is the
  // one that fits anything, so it is the safe guess.
  return "narrow";
}

/**
 * The media query each mode is drawn under, exactly as layout.css writes it.
 *
 * A test holds the stylesheet to these strings, so CSS and script cannot
 * disagree about where a mode starts. The upper bounds stop a hundredth of a
 * pixel short of the next mode, so a fractional width at an odd zoom level
 * falls in exactly one of them.
 */
export function mediaQuery(mode: LayoutMode): string {
  switch (mode) {
    case "wide":
      return `(min-width: ${BREAKPOINTS.wide}px)`;
    case "compact":
      return `(min-width: ${BREAKPOINTS.compact}px) and (max-width: ${BREAKPOINTS.wide - 0.02}px)`;
    case "narrow":
      return `(max-width: ${BREAKPOINTS.compact - 0.02}px)`;
  }
}

/** The sidebar's element id, which every button that opens or closes it names. */
export const SIDEBAR_ID = "sidebar";

/** How the sidebar is shown: in its column, as a rail, as a drawer, or not at all. */
export type SidebarPresentation = "full" | "rail" | "drawer" | "hidden";

/**
 * How the sidebar is shown, given the mode, the operator's collapse preference
 * and whether the drawer is open.
 *
 * The preference belongs to the wide mode only: someone who collapsed the
 * sidebar on a large display has not asked for the drawer to stay shut on a
 * small one. The drawer belongs to the other two, and in the wide mode an open
 * flag left over from a narrower window means nothing.
 */
export function sidebarPresentation(
  mode: LayoutMode,
  collapsed: boolean,
  drawerOpen: boolean,
): SidebarPresentation {
  if (mode === "wide") return collapsed ? "rail" : "full";
  if (drawerOpen) return "drawer";
  return mode === "compact" ? "rail" : "hidden";
}

/** What the sidebar's own button does when pressed. */
export type SidebarAction = "collapse" | "expand" | "open" | "close";

/** The sidebar button, as it should be drawn and announced. */
export interface SidebarToggle {
  action: SidebarAction;
  /** The button's accessible name, and its tooltip. */
  label: string;
  icon: "sidebar-collapse" | "sidebar-expand" | "close";
  /** `aria-expanded`: whether the full sidebar is what is showing now. */
  expanded: boolean;
}

/**
 * The sidebar button for a mode and state.
 *
 * One button, four meanings, because the sidebar has one place for it: in the
 * wide mode it collapses and expands the column; elsewhere it opens and closes
 * the drawer. Narrow mode opens the drawer from the top bar's menu button, so
 * this button is only ever seen there as the drawer's close.
 */
export function sidebarToggle(
  mode: LayoutMode,
  collapsed: boolean,
  drawerOpen: boolean,
): SidebarToggle {
  switch (sidebarPresentation(mode, collapsed, drawerOpen)) {
    case "full":
      return { action: "collapse", label: "Collapse sidebar", icon: "sidebar-collapse", expanded: true };
    case "rail":
      return mode === "wide"
        ? { action: "expand", label: "Expand sidebar", icon: "sidebar-expand", expanded: false }
        : { action: "open", label: "Open navigation", icon: "sidebar-expand", expanded: false };
    case "drawer":
      return { action: "close", label: "Close navigation", icon: "close", expanded: true };
    case "hidden":
      return { action: "open", label: "Open navigation", icon: "sidebar-expand", expanded: false };
  }
}

/** The state after pressing the sidebar button. */
export function applySidebarAction(
  action: SidebarAction,
  state: { collapsed: boolean; drawerOpen: boolean },
): { collapsed: boolean; drawerOpen: boolean } {
  switch (action) {
    case "collapse":
      return { ...state, collapsed: true };
    case "expand":
      return { ...state, collapsed: false };
    case "open":
      return { ...state, drawerOpen: true };
    case "close":
      return { ...state, drawerOpen: false };
  }
}

/** Where the collapse preference is kept between launches. */
export const SIDEBAR_KEY = "agentos.sidebar";

/**
 * The stored preference, read leniently: anything but the one word that means
 * collapsed — including nothing, or something a later version wrote — gives
 * the full sidebar, which is the layout that hides nothing.
 */
export function parseSidebarPreference(stored: string | null): boolean {
  return stored === "collapsed";
}

/** The word stored for a preference. */
export function serialiseSidebarPreference(collapsed: boolean): "collapsed" | "expanded" {
  return collapsed ? "collapsed" : "expanded";
}
