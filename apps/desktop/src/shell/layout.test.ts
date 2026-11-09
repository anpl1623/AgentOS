import { describe, expect, it } from "vitest";

import layoutCss from "../styles/layout.css?raw";
import {
  BREAKPOINTS,
  type LayoutMode,
  applySidebarAction,
  layoutMode,
  mediaQuery,
  parseSidebarPreference,
  serialiseSidebarPreference,
  sidebarPresentation,
  sidebarToggle,
} from "./layout";

const MODES: readonly LayoutMode[] = ["wide", "compact", "narrow"];

describe("layoutMode", () => {
  it("puts each breakpoint in the larger layout", () => {
    expect(layoutMode(BREAKPOINTS.wide)).toBe("wide");
    expect(layoutMode(BREAKPOINTS.wide - 1)).toBe("compact");
    expect(layoutMode(BREAKPOINTS.compact)).toBe("compact");
    expect(layoutMode(BREAKPOINTS.compact - 1)).toBe("narrow");
  });

  it("reaches every layout at a size the window can really be", () => {
    // The Tauri minimum is 940 by 620; 200% zoom halves it.
    expect(layoutMode(1440)).toBe("wide");
    expect(layoutMode(940)).toBe("compact");
    expect(layoutMode(470)).toBe("narrow");
  });

  it("falls to the layout that fits anything when the width cannot be read", () => {
    expect(layoutMode(Number.NaN)).toBe("narrow");
    expect(layoutMode(0)).toBe("narrow");
  });
});

describe("the stylesheet's media queries", () => {
  it.each(MODES)("draws the %s layout under the query the script names", (mode) => {
    expect(layoutCss).toContain(`@media ${mediaQuery(mode)}`);
  });

  it("leaves no width in two layouts or in none", () => {
    // The upper bound of each range sits a fraction below the next one's start.
    expect(mediaQuery("narrow")).toContain(`${BREAKPOINTS.compact - 0.02}px`);
    expect(mediaQuery("compact")).toContain(`${BREAKPOINTS.compact}px`);
    expect(mediaQuery("compact")).toContain(`${BREAKPOINTS.wide - 0.02}px`);
    expect(mediaQuery("wide")).toContain(`${BREAKPOINTS.wide}px`);
  });
});

describe("sidebarPresentation", () => {
  it("shows the wide sidebar full, or as a rail once collapsed", () => {
    expect(sidebarPresentation("wide", false, false)).toBe("full");
    expect(sidebarPresentation("wide", true, false)).toBe("rail");
  });

  it("ignores a drawer flag left over in the wide layout", () => {
    expect(sidebarPresentation("wide", false, true)).toBe("full");
    expect(sidebarPresentation("wide", true, true)).toBe("rail");
  });

  it("shows the compact layout as a rail until its drawer opens", () => {
    expect(sidebarPresentation("compact", false, false)).toBe("rail");
    expect(sidebarPresentation("compact", false, true)).toBe("drawer");
  });

  it("hides the narrow sidebar until its drawer opens", () => {
    expect(sidebarPresentation("narrow", false, false)).toBe("hidden");
    expect(sidebarPresentation("narrow", false, true)).toBe("drawer");
  });

  it("keeps the collapse preference to the wide layout", () => {
    // Collapsed on a big display is not a request for anything elsewhere.
    expect(sidebarPresentation("compact", true, false)).toBe("rail");
    expect(sidebarPresentation("narrow", true, false)).toBe("hidden");
    expect(sidebarPresentation("narrow", true, true)).toBe("drawer");
  });
});

describe("sidebarToggle", () => {
  it("collapses and expands in the wide layout", () => {
    expect(sidebarToggle("wide", false, false)).toEqual({
      action: "collapse",
      label: "Collapse sidebar",
      icon: "sidebar-collapse",
      expanded: true,
    });
    expect(sidebarToggle("wide", true, false)).toMatchObject({
      action: "expand",
      expanded: false,
    });
  });

  it("opens and closes the drawer elsewhere", () => {
    expect(sidebarToggle("compact", false, false)).toMatchObject({
      action: "open",
      label: "Open navigation",
      expanded: false,
    });
    expect(sidebarToggle("compact", false, true)).toMatchObject({
      action: "close",
      icon: "close",
      expanded: true,
    });
    expect(sidebarToggle("narrow", true, true)).toMatchObject({ action: "close" });
  });

  it("says aria-expanded true exactly when the full sidebar is showing", () => {
    for (const mode of MODES) {
      for (const collapsed of [false, true]) {
        for (const drawerOpen of [false, true]) {
          const presentation = sidebarPresentation(mode, collapsed, drawerOpen);
          const full = presentation === "full" || presentation === "drawer";
          expect(sidebarToggle(mode, collapsed, drawerOpen).expanded).toBe(full);
        }
      }
    }
  });

  it("always does something visible when pressed", () => {
    for (const mode of MODES) {
      for (const collapsed of [false, true]) {
        for (const drawerOpen of [false, true]) {
          const before = sidebarPresentation(mode, collapsed, drawerOpen);
          const { action } = sidebarToggle(mode, collapsed, drawerOpen);
          const next = applySidebarAction(action, { collapsed, drawerOpen });
          const after = sidebarPresentation(mode, next.collapsed, next.drawerOpen);
          expect(after, `${mode} collapsed=${collapsed} drawer=${drawerOpen}`).not.toBe(before);
        }
      }
    }
  });
});

describe("applySidebarAction", () => {
  it("changes only the fact the action is about", () => {
    const state = { collapsed: false, drawerOpen: false };
    expect(applySidebarAction("collapse", state)).toEqual({ collapsed: true, drawerOpen: false });
    expect(applySidebarAction("open", state)).toEqual({ collapsed: false, drawerOpen: true });
    expect(applySidebarAction("expand", { collapsed: true, drawerOpen: true })).toEqual({
      collapsed: false,
      drawerOpen: true,
    });
    expect(applySidebarAction("close", { collapsed: true, drawerOpen: true })).toEqual({
      collapsed: true,
      drawerOpen: false,
    });
  });
});

describe("the collapse preference", () => {
  it("round-trips", () => {
    for (const collapsed of [false, true]) {
      expect(parseSidebarPreference(serialiseSidebarPreference(collapsed))).toBe(collapsed);
    }
  });

  it("reads anything unexpected as the full sidebar", () => {
    expect(parseSidebarPreference(null)).toBe(false);
    expect(parseSidebarPreference("")).toBe(false);
    expect(parseSidebarPreference("rail")).toBe(false);
    expect(parseSidebarPreference("COLLAPSED")).toBe(false);
  });
});
