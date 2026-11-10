import { type RefObject, Suspense, useCallback, useEffect, useRef, useState } from "react";

import { AlertHost } from "../components/Alerts";
import { CommandPalette } from "../components/CommandPalette";
import { LeaveConfirmerHost } from "../components/ConfirmDialog";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { installSpotlight } from "../components/motion";
import { ShortcutSheet } from "../components/Shortcuts";
import type { Route } from "../routes/route";
import { useScrollMemory } from "../sdk/cache";
import { startEventStream } from "../sdk/eventStream";
import { ApprovalsAnnouncer } from "./ApprovalsBadge";
import { CloseGuardHost } from "./CloseGuardHost";
import { isApple } from "./keys";
import { applySidebarAction, sidebarPresentation, sidebarToggle } from "./layout";
import { titleFor } from "./nav";
import { useBack, useForward, useNavigate, usePlace } from "./router";
import { Screen, preloadScreens } from "./Screen";
import { ScreenFallback } from "./ScreenFallback";
import { RunFailureAlerts, StoreErrorAlerts } from "./ShellAlerts";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { useLayoutMode, useSidebarCollapsed } from "./useLayout";
import { type KeyOverlay, useShellKeys } from "./useShellKeys";

type Overlay = KeyOverlay | null;

/**
 * The window's chrome: the sidebar, the top bar, the screen, and everything
 * that floats over it.
 *
 * The shell owns where the window is and how a person moves; a screen
 * receives its route's parameters and `navigate` as props and never reaches
 * into the shell. Anything that changes often — the approvals count, the
 * activity feed — is read by a small leaf component, so a new event redraws a
 * badge and not the screen beneath it.
 *
 * This file only composes. The layout's decisions are in `layout.ts`, the
 * sidebar, top bar and drawer each in a module of their own, and the hosts
 * that raise alerts and guard the close beside them.
 */
export function Shell() {
  const { route, hash } = usePlace();
  const navigate = useNavigate();
  const back = useBack();
  const forward = useForward();
  const main = useRef<HTMLElement>(null);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [apple] = useState(() =>
    isApple(typeof navigator === "undefined" ? "" : navigator.platform || navigator.userAgent),
  );

  const mode = useLayoutMode();
  const [collapsed, setCollapsed] = useSidebarCollapsed();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const presentation = sidebarPresentation(mode, collapsed, drawerOpen);
  const drawer = presentation === "drawer";
  const toggle = sidebarToggle(mode, collapsed, drawerOpen);

  useEffect(() => {
    startEventStream();
  }, []);

  // One pointer listener for every lit card in the window.
  useEffect(() => installSpotlight(document), []);

  // Each screen is its own chunk, fetched when first shown; once the window
  // has drawn the one it opened on, the rest are fetched while it is idle.
  useEffect(() => preloadScreens(), []);

  // A drawer opened in one layout is not carried into another: crossing a
  // breakpoint closes it, so widening the window never finds it still open.
  useEffect(() => {
    setDrawerOpen(false);
  }, [mode]);

  // A screen lands where it was left, and a screen not seen before at the top.
  useScrollMemory(hash, main);
  useFocusOnMove(route, hash, main);

  const open = useCallback((next: KeyOverlay) => setOverlay(next), []);
  useShellKeys({ navigate, back, forward, open });

  // Moving from the drawer closes it before the move begins, so the drawer
  // hands focus back first and the new screen then takes it.
  const go = useCallback(
    (target: Route) => {
      setDrawerOpen(false);
      navigate(target);
    },
    [navigate],
  );
  const openFromChrome = useCallback((next: KeyOverlay) => {
    setDrawerOpen(false);
    setOverlay(next);
  }, []);
  const closeOverlay = useCallback(() => setOverlay(null), []);
  const closeDrawer = useCallback(() => setDrawerOpen(false), []);
  const onToggle = () => {
    const next = applySidebarAction(toggle.action, { collapsed, drawerOpen });
    if (next.collapsed !== collapsed) setCollapsed(next.collapsed);
    setDrawerOpen(next.drawerOpen);
  };

  const mod = apple ? "⌘" : "Ctrl+";
  return (
    <div
      className="shell"
      data-collapsed={collapsed ? "true" : undefined}
      data-drawer={drawer ? "open" : undefined}
    >
      <a
        className="skip-link"
        href="#main"
        onClick={(event) => {
          // The address is the route, so following `#main` would leave the
          // screen; move focus instead.
          event.preventDefault();
          main.current?.focus();
        }}
      >
        Skip to content
      </a>

      <TopBar
        current={route.name}
        drawerOpen={drawer}
        onMenu={() => setDrawerOpen(true)}
        onOpenPalette={() => openFromChrome("palette")}
        onNavigate={go}
        apple={apple}
      />

      <Sidebar
        current={route.name}
        presentation={presentation}
        toggle={toggle}
        onToggle={onToggle}
        onNavigate={go}
        onOpenPalette={() => openFromChrome("palette")}
        onOpenShortcuts={() => openFromChrome("shortcuts")}
        onCloseDrawer={closeDrawer}
        apple={apple}
      />

      {/* Behind the drawer: dims the screen, and a press on it closes the
          drawer. The keyboard closes it with Escape, so this is not a control
          of its own. Pressing it keeps focus where it was, for the drawer to
          hand back. */}
      <div
        className="sidebar-scrim"
        aria-hidden="true"
        onMouseDown={(event) => event.preventDefault()}
        onClick={closeDrawer}
      />

      <main id="main" className="main" tabIndex={-1} ref={main} inert={drawer}>
        <div className="content">
          <ErrorBoundary scope="screen" resetKey={hash}>
            {/* A screen whose code is still being fetched shows its frame in
                skeleton. The boundary sits outside the keyed wrapper, so the
                wrapper mounts only once the screen can draw, and rises in
                then rather than half way through a wait; a fetch that fails
                throws to the error boundary above it, as a screen would. */}
            <Suspense fallback={<ScreenFallback />}>
              {/* Keyed on the screen's name, not its address: a new screen
                  rises in, and a change of parameter within one does not
                  remount it. */}
              <div key={route.name} className="screen screen-enter">
                <Screen route={route} navigate={navigate} />
              </div>
            </Suspense>
          </ErrorBoundary>
        </div>
      </main>

      <ApprovalsAnnouncer />
      <AlertHost onNavigate={go} />
      <RunFailureAlerts route={route} />
      <StoreErrorAlerts />

      {overlay === "palette" ? (
        <CommandPalette navigate={navigate} onClose={closeOverlay} modKey={mod} />
      ) : null}
      {overlay === "shortcuts" ? <ShortcutSheet apple={apple} onClose={closeOverlay} /> : null}
      <LeaveConfirmerHost />
      <CloseGuardHost />
    </div>
  );
}

/**
 * On every move: name the window after the screen, and put the keyboard at
 * the top of the new screen. Without the focus, an operator who activates a
 * nav button is left in the sidebar with no sign the content changed. The
 * first screen is not focused, so the skip link is still the first Tab.
 */
function useFocusOnMove(route: Route, hash: string, main: RefObject<HTMLElement | null>): void {
  const shown = useRef<string | null>(null);
  useEffect(() => {
    document.title = titleFor(route);
    if (shown.current !== null && shown.current !== hash) {
      main.current?.focus({ preventScroll: true });
    }
    shown.current = hash;
  }, [route, hash, main]);
}
