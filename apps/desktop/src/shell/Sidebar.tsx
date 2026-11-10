import { useRef } from "react";

import { Icon } from "../components/icons";
import { tip } from "../components/Tooltip";
import type { Route, RouteName } from "../routes/route";
import { Brand } from "./Brand";
import { ConnectionStatus } from "./ConnectionStatus";
import { useDrawer } from "./drawer";
import { SIDEBAR_ID, type SidebarPresentation, type SidebarToggle } from "./layout";
import { NavList } from "./NavList";
import { PaletteLauncher } from "./PaletteLauncher";
import { ThemeToggle } from "./ThemeToggle";

/**
 * The sidebar: the brand and its button, the palette launcher, the screens,
 * and at the foot what the window is connected to and how it is drawn.
 *
 * It is one element in every layout. layout.css decides its geometry — a
 * column, a rail, a drawer over the screen, or off the screen entirely — and
 * the sidebar's container query decides what its contents show at that width,
 * so nothing here branches on the layout but the drawer's semantics: while it
 * is a drawer it is a modal dialog, and keeps the modal contract (`drawer.ts`).
 */
export function Sidebar({
  current,
  presentation,
  toggle,
  onToggle,
  onNavigate,
  onOpenPalette,
  onOpenShortcuts,
  onCloseDrawer,
  apple,
}: {
  current: RouteName;
  presentation: SidebarPresentation;
  toggle: SidebarToggle;
  onToggle: () => void;
  onNavigate: (route: Route) => void;
  onOpenPalette: () => void;
  onOpenShortcuts: () => void;
  onCloseDrawer: () => void;
  apple: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const drawer = presentation === "drawer";
  useDrawer(ref, drawer, onCloseDrawer);

  return (
    <div
      id={SIDEBAR_ID}
      ref={ref}
      className="sidebar"
      // As a drawer it can hold focus itself, out of the Tab order, so a
      // press on a part of it that is no control leaves focus in the drawer
      // rather than dropping it on the body behind.
      {...(drawer
        ? { role: "dialog", "aria-modal": true, "aria-label": "Navigation", tabIndex: -1 }
        : {})}
    >
      <div className="sidebar-head">
        <Brand />
        <button
          type="button"
          className="icon-button sidebar-toggle"
          aria-expanded={toggle.expanded}
          aria-controls={SIDEBAR_ID}
          onClick={onToggle}
          {...tip(toggle.label, "right")}
        >
          <Icon name={toggle.icon} />
        </button>
      </div>

      <PaletteLauncher onOpen={onOpenPalette} apple={apple} />

      <nav className="sidebar-nav" aria-label="Screens">
        <NavList current={current} onNavigate={onNavigate} />
      </nav>

      <div className="sidebar-foot">
        <ConnectionStatus />
        <div className="sidebar-tools">
          <ThemeToggle />
          <button
            type="button"
            className="icon-button shortcuts-button"
            aria-keyshortcuts="?"
            onClick={onOpenShortcuts}
            {...tip("Keyboard shortcuts", "right")}
          >
            <Icon name="command" size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
