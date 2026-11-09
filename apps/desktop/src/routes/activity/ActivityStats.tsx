import type { EventView } from "../../bindings/EventView";
import { StatTile } from "../../components/common";
import { ago, clock } from "../../sdk/format";
import { rateSeries, securityCount, total } from "./feedSignals";

/**
 * Three figures over the shared window, or one run's part of it: how many
 * events, how busy the last hour was, how many were security-relevant, and
 * how long ago the newest arrived.
 *
 * They read the unfiltered stream, not the rows below, so switching the
 * security view or typing a filter does not make them jump. The security tile
 * only asks for attention when something security-relevant happened in the
 * last hour: configuration changes count as security-relevant too, and a tile
 * that glowed for a week-old one would soon be ignored.
 */
export function ActivityStats({
  events,
  loaded,
  now,
  securityOnly,
  onSecurity,
}: {
  events: readonly EventView[];
  /** History has been read, so a zero is a zero rather than not-yet. */
  loaded: boolean;
  now: number;
  securityOnly: boolean;
  onSecurity: () => void;
}) {
  const series = rateSeries(events, now);
  const security = securityCount(events);
  const recentSecurity = total(
    rateSeries(
      events.filter((event) => event.security_relevant),
      now,
    ),
  );
  const latest = events.at(-1);

  return (
    <div className="grid activity-stats stagger">
      <StatTile
        label="Events"
        icon="activity"
        value={loaded ? events.length : null}
        trend={series}
        hint={`${total(series)} in the last hour`}
      />
      <StatTile
        label="Security-relevant"
        icon="shield"
        value={loaded ? security : null}
        tone={recentSecurity > 0 ? "warn" : "accent"}
        hint={
          securityOnly
            ? "Shown on their own below"
            : recentSecurity > 0
              ? `${recentSecurity} in the last hour`
              : "None in the last hour"
        }
        onClick={securityOnly || security === 0 ? undefined : onSecurity}
      />
      <StatTile
        label="Latest"
        icon="clock"
        value={latest ? ago(latest.at) : loaded ? "none yet" : null}
        hint={latest ? clock(latest.at) : "Nothing recorded"}
      />
    </div>
  );
}
