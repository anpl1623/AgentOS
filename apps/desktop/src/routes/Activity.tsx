import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Empty, ErrorBanner, Loading, SkeletonRows } from "../components/common";
import { useCopy, useNow } from "../components/hooks";
import type { EventView } from "../bindings/EventView";
import { api } from "../sdk/client";
import { reloadEventHistory, setFeedFilter, useEvents, useFeedFilter } from "../sdk/eventStream";
import { clock } from "../sdk/format";
import { useAsync } from "../sdk/useAsync";
import {
  DEEP_LOAD,
  SHALLOW_LOAD,
  USER_SCROLL_WINDOW_MS,
  canOpen,
  dayLabel,
  emptyMessage,
  feedNote,
  filterFeed,
  followAfterScroll,
  groupByDay,
  isFiltering,
  kindOptions,
  latestSequence,
  mergeFeed,
  shortHash,
  toJsonl,
} from "./activityModel";
import type { Navigate } from "./route";

const RECORD_ID = "activity-record";
const RECORD_TITLE_ID = "activity-record-title";

/**
 * Everything the runtime has recorded, as it happens.
 *
 * A view onto the hash-chained log, not the log: the feed is the shared stream
 * the shell keeps for the life of the window, plus whatever this screen reads
 * back. Security only asks the log itself, because a filter over a held
 * window can report nothing while refusals sit just outside it.
 */
export function Activity({
  runId,
  navigate,
}: {
  /** One run to narrow the feed to, from the address. */
  runId?: string | undefined;
  navigate: Navigate;
}) {
  const stream = useEvents();
  const { securityOnly, follow } = useFeedFilter();
  const [depth, setDepth] = useState(SHALLOW_LOAD);
  const [text, setText] = useState("");
  const [kind, setKind] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const { copy, state: copyState } = useCopy();
  // Minute resolution is enough for "Today" to become "Yesterday" at midnight.
  const today = new Date(useNow());

  // The unfiltered view is the shared stream, which reads its own history;
  // only the security view asks the log from here.
  const history = useAsync<EventView[] | null>(
    () => (securityOnly ? api.activity(depth, true) : Promise.resolve(null)),
    [securityOnly, depth],
  );
  const [deepening, setDeepening] = useState(false);

  const held = useMemo(
    () => mergeFeed([securityOnly ? stream.security : stream.all, history.data ?? []]),
    [securityOnly, stream.security, stream.all, history.data],
  );
  const query = { text, kind, runId: runId ?? null };
  // Keyed on the query's fields: the object itself is new on every render.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const shown = useMemo(() => filterFeed(held, query), [held, text, kind, runId]);
  const groups = useMemo(() => groupByDay(shown), [shown]);
  const kinds = useMemo(() => kindOptions(held, kind), [held, kind]);
  const unwritten = shown.filter((event) => !canOpen(event)).length;

  const failed = securityOnly ? history.error !== null : stream.error !== null;
  const loading = securityOnly ? history.loading : !stream.loaded && stream.error === null;
  const note = feedNote({
    securityOnly,
    held: held.length,
    latest: latestSequence(held),
    requested: depth,
    received: securityOnly ? (history.data?.length ?? null) : null,
    read: stream.depth,
    runId: runId ?? null,
  });

  const feedEnd = useFollow(follow, shown.at(-1)?.id);

  // The log region speaks while Follow is on, and it speaks every row it
  // gains. A history read, a filter or the security switch can put hundreds
  // in at once, none of them new, so each is made with the region silent.
  const { hushed, hush, release } = useHush();
  const awaitingSecurity = useRef(false);
  useEffect(() => {
    if (!awaitingSecurity.current) return;
    // Turning the security view on waits for its first read of the log;
    // turning it off has already swapped the rows.
    if (securityOnly && history.data === null && history.error === null) return;
    awaitingSecurity.current = false;
    release();
  }, [securityOnly, history.data, history.error, release]);
  const filterBy = (change: () => void) => {
    hush();
    change();
    release();
  };

  const loadMore = () => {
    if (securityOnly) {
      if (depth === DEEP_LOAD) history.reload();
      else {
        hush();
        awaitingSecurity.current = true;
        setDepth(DEEP_LOAD);
      }
      return;
    }
    // Into the shared stream rather than this screen's state, so the deeper
    // window is still there after the operator leaves and comes back.
    setDeepening(true);
    hush();
    void reloadEventHistory(DEEP_LOAD).finally(() => {
      setDeepening(false);
      release();
    });
  };

  const readAgain = () => {
    hush();
    void reloadEventHistory().finally(release);
  };

  // Focus goes back to the row that opened the record, so a keyboard user
  // closing it is where they were in the feed, not at the top of the page.
  const feed = useRef<HTMLDivElement>(null);
  const closeRecord = () => {
    const opener = openId;
    setOpenId(null);
    const row =
      opener === null
        ? null
        : feed.current?.querySelector<HTMLElement>(`[data-event-id="${CSS.escape(opener)}"]`);
    (row ?? feed.current)?.focus();
  };

  return (
    <>
      <div className="page-head">
        <h1>Activity</h1>
        <div className="inline">
          <button
            type="button"
            aria-pressed={securityOnly}
            onClick={() => {
              hush();
              awaitingSecurity.current = true;
              setFeedFilter({ securityOnly: !securityOnly });
            }}
          >
            Security only
          </button>
          <button
            type="button"
            aria-pressed={follow}
            onClick={() => {
              // An explicit override: on re-engages and jumps, whatever the
              // scroll position last said.
              setFeedFilter({ follow: !follow });
              if (!follow) feedEnd.current?.scrollIntoView({ block: "end" });
            }}
          >
            Follow
          </button>
          <button
            type="button"
            disabled={shown.length === 0}
            onClick={() => void copy(toJsonl(shown))}
          >
            {copyState === "copied"
              ? `Copied ${shown.length}`
              : copyState === "failed"
                ? "Copy failed"
                : "Copy visible"}
          </button>
          <button
            type="button"
            disabled={deepening || history.refreshing || history.loading}
            onClick={loadMore}
          >
            Load {DEEP_LOAD}
          </button>
        </div>
      </div>
      <p className="page-sub">
        Every action, permission decision, refusal and escalation, in the order the log recorded it.
      </p>

      <div className="toolbar">
        <input
          type="search"
          aria-label="Filter by kind or summary"
          placeholder="Filter by kind or summary"
          value={text}
          onChange={(event) => filterBy(() => setText(event.target.value))}
        />
        <select
          aria-label="Kind"
          value={kind}
          onChange={(event) => filterBy(() => setKind(event.target.value))}
        >
          <option value="">Every kind</option>
          {kinds.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={!isFiltering(query)}
          onClick={() =>
            filterBy(() => {
              setText("");
              setKind("");
            })
          }
        >
          Clear
        </button>
        {runId ? (
          <span className="inline">
            <span className="tag" title={runId}>
              run {runId}
            </span>
            <button
              type="button"
              className="ghost"
              aria-label="Show every run"
              onClick={() => navigate({ name: "activity" })}
            >
              ✕
            </button>
          </span>
        ) : null}
        {/* Not live: it changes with every arriving event, and the log
         * region beside it already speaks while Follow is on. */}
        <span className="faint">
          {shown.length} of {held.length}
        </span>
      </div>

      {history.error ? <ErrorBanner message={history.error} /> : null}
      {!securityOnly && stream.error ? <ErrorBanner message={stream.error} /> : null}

      <div
        ref={feed}
        className="panel feed"
        role="log"
        tabIndex={-1}
        aria-label={securityOnly ? "Security-relevant events" : "Activity"}
        aria-live={follow && !hushed ? "polite" : "off"}
      >
        {shown.length === 0 && loading ? (
          <>
            <Loading what={securityOnly ? "security records" : "activity"} />
            <SkeletonRows count={4} />
          </>
        ) : null}
        {shown.length === 0 && !loading ? (
          <Empty>
            {emptyMessage({
              securityOnly,
              filtering: isFiltering(query),
              runId: runId ?? null,
              failed,
            })}
          </Empty>
        ) : null}
        {groups.map((group) => {
          const label = dayLabel(group.day, today);
          return (
            <section key={group.id} aria-label={label}>
              {/* Roles rather than ul and li: the semantics are the same to
               * assistive technology, and the elements bring indents and
               * bullets that no stylesheet here resets. */}
              <h2 className="event-date">{label}</h2>
              <div role="list">
                {group.events.map((event) => (
                  <div role="listitem" key={event.id}>
                    <EventRow
                      event={event}
                      open={openId === event.id}
                      onOpen={() =>
                        setOpenId((current) => (current === event.id ? null : event.id))
                      }
                    />
                  </div>
                ))}
              </div>
            </section>
          );
        })}
        <div ref={feedEnd} />
      </div>

      {note ? <p className="field-note">{note}</p> : null}
      {unwritten > 0 ? (
        <p className="field-note">
          {unwritten === 1 ? "One event" : `${unwritten} events`} arrived after history was read and{" "}
          {unwritten === 1 ? "is" : "are"} still being written to the chain, so{" "}
          {unwritten === 1 ? "it opens" : "they open"} once history is read again.{" "}
          <button type="button" className="ghost" onClick={readAgain}>
            Read history again
          </button>
        </p>
      ) : null}

      {openId !== null ? (
        <Record id={openId} navigate={navigate} onClose={closeRecord} />
      ) : null}
    </>
  );
}

/**
 * One line of the feed.
 *
 * A stored event is a button that opens its record. A streamed one is not
 * yet in the log by id, so it is plain text rather than a control that would
 * answer with an error.
 */
function EventRow({
  event,
  open,
  onOpen,
}: {
  event: EventView;
  open: boolean;
  onOpen: () => void;
}) {
  const security = event.security_relevant ? " security" : "";
  // The flag and the amber kind are what a sighted operator sees; the hidden
  // prefix is the same fact for everyone else, since colour carries nothing
  // to a screen reader.
  const content = (
    <>
      <span className="event-time">{clock(event.at)}</span>
      {event.security_relevant ? (
        <span className="visually-hidden">security relevant: </span>
      ) : null}
      <span className="event-flag" aria-hidden="true">
        {event.security_relevant ? "!" : ""}
      </span>
      <span className="event-kind">{event.kind}</span>
      <span className="event-summary" title={event.summary}>
        {event.summary}
      </span>
    </>
  );
  if (!canOpen(event)) {
    return (
      <div className={`row event${security}`} title="Still being written to the chain">
        {content}
      </div>
    );
  }
  return (
    <button
      type="button"
      className={`row clickable event${security}`}
      data-event-id={event.id}
      aria-expanded={open}
      aria-controls={open ? RECORD_ID : undefined}
      onClick={onOpen}
    >
      {content}
    </button>
  );
}

/**
 * One record as the chain stores it: the payload in full and both hashes.
 *
 * Focus moves to its heading when it opens, so a keyboard operator who opened
 * it from deep in the feed is taken to what they asked for.
 */
function Record({
  id,
  navigate,
  onClose,
}: {
  id: string;
  navigate: Navigate;
  onClose: () => void;
}) {
  const record = useAsync(() => api.auditRecord(id), [id]);
  const heading = useRef<HTMLHeadingElement>(null);
  const data = record.data;
  const runId = data?.run_id ?? null;

  useEffect(() => {
    heading.current?.scrollIntoView({ block: "start" });
    heading.current?.focus({ preventScroll: true });
  }, [id]);

  return (
    <section id={RECORD_ID} aria-labelledby={RECORD_TITLE_ID}>
      <div className="page-head">
        <h2 id={RECORD_TITLE_ID} ref={heading} tabIndex={-1}>
          {data ? `#${data.sequence} · ${data.kind} · ${clock(data.at)}` : "Record"}
        </h2>
        <div className="inline">
          {runId !== null ? (
            <button type="button" onClick={() => navigate({ name: "tasks", runId })}>
              Open run →
            </button>
          ) : null}
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
      {record.error ? <ErrorBanner message={record.error} /> : null}
      {record.loading ? <Loading what="record" /> : null}
      {data ? (
        <div className="panel">
          <div className="panel-body">
            <dl className="facts">
              <dt>Record before it</dt>
              <dd className="mono" title={data.prev_hash}>
                {shortHash(data.prev_hash)}
              </dd>
              <dt>This record</dt>
              <dd className="mono" title={data.hash}>
                {shortHash(data.hash)}
              </dd>
            </dl>
            <pre className="code" tabIndex={0} role="region" aria-label="Record payload">
              {data.payload}
            </pre>
            <p className="field-note">
              This record's hash covers the payload above — the same JSON, laid out for reading —
              with its position, time and the hash before it, so a record edited after it was
              written no longer matches, and neither does anything after it.
            </p>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/**
 * Silence a live region through deliberate changes to what it holds.
 *
 * `hush` before the change, `release` once it has been made. The region
 * speaks again one commit after the last release, never in the commit that
 * changed its rows, so what a release lets through is only what arrives
 * afterwards. Counted, so one change finishing does not unsilence another
 * still in progress.
 */
function useHush() {
  const [pending, setPending] = useState(0);
  const [finished, setFinished] = useState(0);
  useEffect(() => {
    if (finished === 0) return;
    setPending((count) => Math.max(0, count - finished));
    setFinished(0);
  }, [finished]);
  const hush = useCallback(() => setPending((count) => count + 1), []);
  const release = useCallback(() => setFinished((count) => count + 1), []);
  return { hushed: pending > 0, hush, release };
}

/**
 * Keep the newest event in view while Follow is on, and let the operator's
 * own scrolling switch it.
 *
 * Follow fought the operator when it was only a toggle: it kept pulling the
 * page to the bottom while they read. Now a scroll they make more than the
 * threshold away from the end turns it off and a scroll back turns it on.
 * Scrolls the screen makes itself — the jump to the newest event, the shell
 * restoring a remembered position — are told apart by whether a wheel, touch,
 * key or held pointer just happened, and change nothing.
 *
 * Returns the marker at the end of the feed.
 */
function useFollow(follow: boolean, newest: string | undefined) {
  const end = useRef<HTMLDivElement>(null);
  const followRef = useRef(follow);
  followRef.current = follow;

  useEffect(() => {
    const marker = end.current;
    if (marker === null) return;
    const scroller = scrollContainer(marker);
    const target: HTMLElement | Window = scroller ?? window;
    let lastInput = Number.NEGATIVE_INFINITY;
    let pointerDown = false;

    const input = () => {
      lastInput = performance.now();
    };
    const down = () => {
      pointerDown = true;
      input();
    };
    const up = () => {
      pointerDown = false;
    };
    const scroll = () => {
      const byOperator = pointerDown || performance.now() - lastInput < USER_SCROLL_WINDOW_MS;
      const bottom = scroller ? scroller.getBoundingClientRect().bottom : window.innerHeight;
      const distance = marker.getBoundingClientRect().bottom - bottom;
      const next = followAfterScroll(followRef.current, distance, byOperator);
      if (next !== followRef.current) setFeedFilter({ follow: next });
    };

    const inputs = ["wheel", "touchmove", "keydown"] as const;
    for (const kind of inputs) target.addEventListener(kind, input, { passive: true });
    target.addEventListener("pointerdown", down, { passive: true });
    window.addEventListener("pointerup", up, { passive: true });
    window.addEventListener("pointercancel", up, { passive: true });
    target.addEventListener("scroll", scroll, { passive: true });
    return () => {
      for (const kind of inputs) target.removeEventListener(kind, input);
      target.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      target.removeEventListener("scroll", scroll);
    };
  }, []);

  // A passive effect, so on arrival it runs after the shell has restored the
  // screen's remembered position; Follow, when on, has the last word.
  useEffect(() => {
    if (follow) end.current?.scrollIntoView({ block: "end" });
  }, [follow, newest]);

  return end;
}

/** The nearest ancestor that scrolls, or `null` when the document does. */
function scrollContainer(element: HTMLElement): HTMLElement | null {
  for (let at = element.parentElement; at !== null; at = at.parentElement) {
    const { overflowY } = getComputedStyle(at);
    if (overflowY === "auto" || overflowY === "scroll") return at;
  }
  return null;
}
