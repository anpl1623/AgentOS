import type { Route } from "../routes/route";
import { type Alert, dismiss, useAlerts } from "../sdk/alerts";

/**
 * The stack of alerts in the window's corner.
 *
 * Warnings and errors are `role="alert"` and interrupt; a note is
 * `role="status"` and waits its turn. Every card can be dismissed, with a real
 * button that a keyboard reaches, because a sticky card nobody can close is a
 * card that teaches people to ignore the corner. The stack sits below the
 * palette and dialogs, so an alert never covers a question being asked.
 *
 * A card's only action is its link, and following it is all the host does with
 * it: `onNavigate` is the shell's, which is what can move the window.
 */
export function AlertHost({ onNavigate }: { onNavigate: (route: Route) => void }) {
  const alerts = useAlerts();
  return (
    <div className="alerts">
      {alerts.map((alert) => (
        <AlertCard key={alert.id} alert={alert} onNavigate={onNavigate} />
      ))}
    </div>
  );
}

function AlertCard({
  alert,
  onNavigate,
}: {
  alert: Alert;
  onNavigate: (route: Route) => void;
}) {
  const { link } = alert;
  return (
    <div className={`alert ${alert.level}`} role={alert.level === "info" ? "status" : "alert"}>
      <div className="alert-title">{alert.message}</div>
      {alert.detail !== null ? <div className="alert-detail">{alert.detail}</div> : null}
      <div className="alert-actions">
        {link !== null ? (
          <button
            type="button"
            onClick={() => {
              dismiss(alert.id);
              onNavigate(link.route);
            }}
          >
            {link.label}
          </button>
        ) : null}
        <button
          type="button"
          className="ghost"
          aria-label={`Dismiss: ${alert.message}`}
          onClick={() => dismiss(alert.id)}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
