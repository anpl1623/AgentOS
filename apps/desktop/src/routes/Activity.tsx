import { useEffect, useRef, useState } from "react";

import { ErrorBanner, Empty, Loading } from "../components/common";
import type { EventView } from "../bindings/EventView";
import { api, events } from "../sdk/client";
import { clock } from "../sdk/format";
import { subscribe } from "../sdk/transport";
import { useAsync } from "../sdk/useAsync";

/** How many events to keep on screen before dropping the oldest. */
const WINDOW = 500;

/**
 * Everything the runtime has recorded, as it happens.
 *
 * Loads recent history, then appends live events rather than re-fetching — the
 * feed is the one place where seeing the moment something occurred is the point.
 * The durable log remains the source of truth; this is a view onto it.
 */
export function Activity() {
  const history = useAsync(() => api.activity(200), []);
  const [live, setLive] = useState<EventView[]>([]);
  const [securityOnly, setSecurityOnly] = useState(false);
  const bottom = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);

  useEffect(() => {
    const subscription = subscribe<EventView>(events.activity, (event) => {
      setLive((current) => [...current, event].slice(-WINDOW));
    });
    return () => {
      void subscription.then((unsubscribe) => unsubscribe());
    };
  }, []);

  const all = [...(history.data ?? []), ...live].slice(-WINDOW);
  const shown = securityOnly ? all.filter((event) => event.security_relevant) : all;

  useEffect(() => {
    if (follow) bottom.current?.scrollIntoView({ block: "end" });
  }, [shown.length, follow]);

  return (
    <>
      <div className="page-head">
        <h1>Activity</h1>
        <div className="inline">
          <button
            type="button"
            className={securityOnly ? "primary" : ""}
            onClick={() => setSecurityOnly((value) => !value)}
          >
            Refusals only
          </button>
          <button type="button" className={follow ? "primary" : ""} onClick={() => setFollow((v) => !v)}>
            Follow
          </button>
        </div>
      </div>
      <p className="page-sub">
        Every action, permission decision and refusal, in the order it happened.
      </p>

      {history.error ? <ErrorBanner message={history.error} /> : null}
      {history.loading && !history.data ? <Loading what="activity" /> : null}

      <div className="panel">
        {shown.length === 0 ? (
          <Empty>{securityOnly ? "Nothing has been refused." : "Nothing recorded yet."}</Empty>
        ) : null}
        {shown.map((event, index) => (
          <div
            key={`${event.id}-${index}`}
            className={`event ${event.security_relevant ? "security" : ""}`}
          >
            <span className="event-time">{clock(event.at)}</span>
            <span className="event-kind">{event.kind}</span>
            <span className="event-summary">{event.summary}</span>
          </div>
        ))}
        <div ref={bottom} />
      </div>
    </>
  );
}
