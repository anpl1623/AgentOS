import type { CSSProperties } from "react";

import type { Route } from "../routes/route";
import { ALERT_TTL_MS, type Alert, type AlertLevel, dismiss, useAlerts } from "../sdk/alerts";
import { Icon, type IconName } from "./icons";

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

/** The mark each level leads with, beside the colour that also says it. */
const LEVEL_ICON: Record<AlertLevel, IconName> = {
  info: "info",
  warn: "alert",
  error: "x",
};

/**
 * Each raise of an alert, numbered. Raising again with the same id replaces
 * the card in place and restarts its expiry, so its countdown is keyed on the
 * raise rather than the id, and starts again with it.
 */
const raises = new WeakMap<Alert, number>();
let raiseCount = 0;
function raiseOf(alert: Alert): number {
  let serial = raises.get(alert);
  if (serial === undefined) {
    raiseCount += 1;
    serial = raiseCount;
    raises.set(alert, serial);
  }
  return serial;
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
      <div className="alert-row">
        <span className="alert-icon" aria-hidden="true">
          <Icon name={LEVEL_ICON[alert.level]} size={18} />
        </span>
        <div className="alert-body">
          <div className="alert-title">{alert.message}</div>
          {alert.detail !== null ? <div className="alert-detail">{alert.detail}</div> : null}
          {link !== null ? (
            <div className="alert-actions">
              <button
                type="button"
                onClick={() => {
                  dismiss(alert.id);
                  onNavigate(link.route);
                }}
              >
                {link.label}
                <Icon name="chevron-right" size={14} />
              </button>
            </div>
          ) : null}
        </div>
        <button
          type="button"
          className="icon-button alert-dismiss"
          aria-label={`Dismiss: ${alert.message}`}
          onClick={() => dismiss(alert.id)}
        >
          <Icon name="close" size={16} />
        </button>
      </div>
      {/* How long a card that leaves by itself has left. A sticky card stays
          until it is dismissed, so it has no clock. */}
      {alert.sticky ? null : (
        <span
          key={raiseOf(alert)}
          className="alert-countdown"
          aria-hidden="true"
          style={{ "--alert-ttl": `${ALERT_TTL_MS}ms` } as CSSProperties}
        />
      )}
    </div>
  );
}
