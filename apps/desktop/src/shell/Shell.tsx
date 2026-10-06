import { memo, useCallback, useEffect, useRef, useState } from "react";

import type { EventView } from "../bindings/EventView";
import { AlertHost } from "../components/Alerts";
import { CommandPalette } from "../components/CommandPalette";
import { LeaveConfirmerHost } from "../components/ConfirmDialog";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { ShortcutSheet } from "../components/Shortcuts";
import { Activity } from "../routes/Activity";
import { Agents } from "../routes/Agents";
import { Approvals } from "../routes/Approvals";
import { Dashboard } from "../routes/Dashboard";
import type { Navigate, Route } from "../routes/route";
import { Schedules } from "../routes/Schedules";
import { Settings } from "../routes/Settings";
import { Tasks } from "../routes/Tasks";
import { dismiss, raise } from "../sdk/alerts";
import { usePendingApprovals } from "../sdk/approvals";
import { useScrollMemory } from "../sdk/cache";
import { events } from "../sdk/client";
import { startEventStream, useEvents } from "../sdk/eventStream";
import { useLive } from "../sdk/live";
import { usingFixtures } from "../sdk/transport";
import { commandFor, isApple, isTypingTarget } from "./keys";
import { modalOpen } from "./modal";
import { NAV, screenRoute, titleFor } from "./nav";
import { type StoreName, alertForEvent, alertForStoreError, storeAlertId } from "./notices";
import { useBack, useForward, useNavigate, usePlace } from "./router";

type Overlay = "palette" | "shortcuts" | null;

/**
 * The window's chrome: the sidebar, the screen, and everything that floats
 * over it.
 *
 * The shell owns where the window is and how a person moves; a screen
 * receives its route's parameters and `navigate` as props and never reaches
 * into the shell. Anything that changes often — the approvals count, the
 * activity feed — is read by a small leaf component, so a new event redraws a
 * badge and not the screen beneath it.
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

  useEffect(() => {
    startEventStream();
  }, []);

  // A screen lands where it was left, and a screen not seen before at the top.
  useScrollMemory(hash, main);

  // On every move: name the window after the screen, and put the keyboard at
  // the top of the new screen. Without the focus, an operator who activates a
  // nav button is left in the sidebar with no sign the content changed. The
  // first screen is not focused, so the skip link is still the first Tab.
  const shown = useRef<string | null>(null);
  useEffect(() => {
    document.title = titleFor(route);
    if (shown.current !== null && shown.current !== hash) {
      main.current?.focus({ preventScroll: true });
    }
    shown.current = hash;
  }, [route, hash]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || modalOpen()) return;
      const command = commandFor(event, isTypingTarget(event.target));
      if (command === null) return;
      event.preventDefault();
      switch (command.kind) {
        case "screen":
          navigate(screenRoute(command.route));
          break;
        case "palette":
          setOverlay("palette");
          break;
        case "shortcuts":
          setOverlay("shortcuts");
          break;
        case "back":
          back();
          break;
        case "forward":
          forward();
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate, back, forward]);

  const closeOverlay = useCallback(() => setOverlay(null), []);

  const mod = apple ? "⌘" : "Ctrl+";
  return (
    <div className="shell">
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

      <nav className="sidebar" aria-label="Screens">
        <div className="brand">
          <Mark />
          AgentOS
        </div>

        {NAV.map((item) => {
          const active = route.name === item.route;
          return (
            <button
              key={item.route}
              type="button"
              className={active ? "nav-item active" : "nav-item"}
              aria-current={active ? "page" : undefined}
              onClick={() => navigate(screenRoute(item.route))}
            >
              <span>{item.label}</span>
              {item.route === "approvals" ? <ApprovalsCount /> : null}
            </button>
          );
        })}

        <ApprovalsAnnouncer />

        <div
          className="sidebar-foot"
          title={`${mod}K to go anywhere, ${mod}1–${NAV.length} for a screen, ? for every shortcut`}
        >
          {usingFixtures() ? (
            <span className="fixture-warning" role="status">
              <span aria-hidden="true">⚠ </span>
              Fixture data — not connected to a runtime
            </span>
          ) : (
            <span>Local runtime</span>
          )}
        </div>
      </nav>

      <main id="main" className="main" tabIndex={-1} ref={main}>
        <ErrorBoundary scope="screen" resetKey={hash}>
          <Screen route={route} navigate={navigate} />
        </ErrorBoundary>
      </main>

      <AlertHost onNavigate={navigate} />
      <RunFailureAlerts route={route} />
      <StoreErrorAlerts />

      {overlay === "palette" ? (
        <CommandPalette navigate={navigate} onClose={closeOverlay} modKey={mod} />
      ) : null}
      {overlay === "shortcuts" ? <ShortcutSheet apple={apple} onClose={closeOverlay} /> : null}
      <LeaveConfirmerHost />
    </div>
  );
}

/** The screen for a route, with the props its route implies. */
const Screen = memo(function Screen({ route, navigate }: { route: Route; navigate: Navigate }) {
  switch (route.name) {
    case "dashboard":
      return <Dashboard navigate={navigate} />;
    case "approvals":
      return <Approvals focus={route.focus} />;
    case "tasks":
      return <Tasks route={route} navigate={navigate} />;
    case "schedules":
      return <Schedules />;
    case "agents":
      return <Agents route={route} navigate={navigate} />;
    case "activity":
      return <Activity runId={route.runId} navigate={navigate} />;
    case "settings":
      return <Settings navigate={navigate} />;
  }
});

/**
 * The count on the Approvals nav item, read from the shared queue so the badge
 * and the cards cannot disagree.
 *
 * When the runtime cannot be read the count is kept and marked stale: dropping
 * it to nothing would tell the operator nothing is waiting on them, which is
 * the wrong failure here. It is announced by {@link ApprovalsAnnouncer}, not
 * here: this sits inside a button, whose content is presentational, and a live
 * region there is not reliably spoken.
 */
function ApprovalsCount() {
  const { approvals, error } = usePendingApprovals();
  const count = approvals.length;
  return (
    <span className="inline">
      {count > 0 ? (
        <span className="nav-count">
          {count}
          <span className="visually-hidden"> waiting on you</span>
        </span>
      ) : null}
      {error !== null ? (
        <span className="stale">
          stale
          <span className="visually-hidden">: the runtime did not answer the last check</span>
        </span>
      ) : null}
    </span>
  );
}

/**
 * Speaks the approvals count when it changes, so a request raised while the
 * operator is on another screen is heard, not only drawn.
 *
 * Always present, because a region that appears with its first number is not
 * announced, and outside the nav button for the reason {@link ApprovalsCount}
 * gives.
 */
function ApprovalsAnnouncer() {
  const count = usePendingApprovals().approvals.length;
  return (
    <span className="visually-hidden" aria-live="polite">
      {count === 0 ? "" : count === 1 ? "1 approval waiting on you" : `${count} approvals waiting on you`}
    </span>
  );
}

/**
 * Raises a failed run, as it is streamed. Only live events raise: history read
 * on start is the record, not news.
 */
function RunFailureAlerts({ route }: { route: Route }) {
  useLive<EventView>(events.activity, (event) => {
    const alert = alertForEvent(event, route);
    if (alert !== null) raise(alert);
  });
  return null;
}

/** Shows a store's failure to reach the runtime, and takes it down on recovery. */
function StoreErrorAlerts() {
  useStoreAlert("approvals", usePendingApprovals().error);
  useStoreAlert("events", useEvents().error);
  return null;
}

function useStoreAlert(store: StoreName, error: string | null): void {
  // Raised once per distinct failure: a person who dismissed it is not shown
  // the same words again on every failed poll.
  useEffect(() => {
    if (error === null) dismiss(storeAlertId(store));
    else raise(alertForStoreError(store, error));
  }, [store, error]);
}

/** The shield from the application icon, so the chrome and the dock agree. */
function Mark() {
  return (
    <svg className="brand-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M4.5 3.5h15v9.2c0 4.6-3.4 7.8-7.5 7.8s-7.5-3.2-7.5-7.8V3.5z"
        className="brand-mark-outer"
        fill="none"
        strokeWidth="2.4"
      />
      <circle
        className="brand-mark-inner"
        cx="12"
        cy="10.6"
        r="2.9"
        fill="none"
        strokeWidth="1.7"
      />
      <circle className="brand-mark-dot" cx="12" cy="10.6" r="0.9" />
    </svg>
  );
}
