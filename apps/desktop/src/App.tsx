import { useCallback, useEffect, useState } from "react";

import { Activity } from "./routes/Activity";
import { Agents } from "./routes/Agents";
import { Approvals } from "./routes/Approvals";
import { Dashboard } from "./routes/Dashboard";
import { Settings } from "./routes/Settings";
import { Tasks } from "./routes/Tasks";
import { api, events } from "./sdk/client";
import { subscribe, usingFixtures } from "./sdk/transport";
import type { Route } from "./routes/route";

const NAV: { route: Route["name"]; label: string }[] = [
  { route: "dashboard", label: "Dashboard" },
  { route: "approvals", label: "Approvals" },
  { route: "tasks", label: "Tasks" },
  { route: "agents", label: "Agents" },
  { route: "activity", label: "Activity" },
  { route: "settings", label: "Settings" },
];

export function App() {
  const [route, setRoute] = useState<Route>({ name: "dashboard" });
  const [pending, setPending] = useState(0);

  const navigate = useCallback((next: Route) => setRoute(next), []);

  // The approvals count sits in the chrome, so it has to be right on every
  // screen. It is refreshed from the runtime rather than counted locally:
  // a run can raise or resolve an approval while the operator is looking
  // somewhere else entirely.
  const refreshPending = useCallback(() => {
    api
      .listPendingApprovals()
      .then((list) => setPending(list.length))
      .catch(() => setPending(0));
  }, []);

  useEffect(() => {
    refreshPending();
    const timer = setInterval(refreshPending, 5000);
    return () => clearInterval(timer);
  }, [refreshPending]);

  useEffect(() => {
    const subscriptions = [
      subscribe(events.approvalRequested, refreshPending),
      subscribe(events.approvalResolved, refreshPending),
    ];
    return () => {
      for (const pendingUnsubscribe of subscriptions) {
        void pendingUnsubscribe.then((unsubscribe) => unsubscribe());
      }
    };
  }, [refreshPending]);

  return (
    <div className="shell">
      <nav className="sidebar">
        <div className="brand">
          <Mark />
          AgentOS
        </div>

        {NAV.map((item) => (
          <button
            key={item.route}
            type="button"
            className={`nav-item ${route.name === item.route ? "active" : ""}`}
            onClick={() => navigate({ name: item.route } as Route)}
          >
            <span>{item.label}</span>
            {item.route === "approvals" && pending > 0 ? (
              <span className="nav-count">{pending}</span>
            ) : null}
          </button>
        ))}

        <div className="sidebar-foot">
          {usingFixtures() ? (
            <span style={{ color: "var(--warn)" }}>
              Fixture data — not connected to a runtime
            </span>
          ) : (
            <span>Local runtime</span>
          )}
        </div>
      </nav>

      <main className="main">
        {route.name === "dashboard" ? <Dashboard navigate={navigate} /> : null}
        {route.name === "approvals" ? <Approvals onResolved={refreshPending} /> : null}
        {route.name === "tasks" ? <Tasks route={route} navigate={navigate} /> : null}
        {route.name === "agents" ? <Agents route={route} navigate={navigate} /> : null}
        {route.name === "activity" ? <Activity /> : null}
        {route.name === "settings" ? <Settings /> : null}
      </main>
    </div>
  );
}

/** The shield from the application icon, so the chrome and the dock agree. */
function Mark() {
  return (
    <svg className="brand-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M4.5 3.5h15v9.2c0 4.6-3.4 7.8-7.5 7.8s-7.5-3.2-7.5-7.8V3.5z"
        fill="none"
        stroke="#2f6de0"
        strokeWidth="2.4"
      />
      <circle cx="12" cy="10.6" r="2.9" fill="none" stroke="#e8f0ff" strokeWidth="1.7" />
      <circle cx="12" cy="10.6" r="0.9" fill="#e8f0ff" />
    </svg>
  );
}
