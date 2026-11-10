import { Icon } from "../components/icons";
import { LiveDot } from "../components/LiveDot";
import { useEvents } from "../sdk/eventStream";
import { usingFixtures } from "../sdk/transport";

/**
 * What the window is connected to, at the foot of the sidebar.
 *
 * Fixture data is said plainly and in the attention hue, because a screen of
 * invented approvals must never be mistaken for real ones. Otherwise a live
 * dot shows the runtime is answering; when the event feed's last read failed
 * the dot stops pulsing and turns to attention. The words stay in the DOM in
 * the rail, where the eye gets them from the tooltip.
 */
export function ConnectionStatus() {
  if (usingFixtures()) {
    return (
      <span
        className="connection fixture-warning"
        role="status"
        data-tip="Fixture data"
        data-tip-side="right"
      >
        <Icon name="alert" size={16} />
        <span className="connection-label">Fixture data — not connected to a runtime</span>
      </span>
    );
  }
  return <RuntimeStatus />;
}

function RuntimeStatus() {
  const failing = useEvents().error !== null;
  const words = failing ? "Runtime not answering" : "Local runtime";
  return (
    <span
      className={failing ? "connection failing" : "connection"}
      data-tip={words}
      data-tip-side="right"
    >
      <LiveDot tone={failing ? "warn" : "ok"} pulse={!failing} />
      <span className="connection-label">{words}</span>
    </span>
  );
}
