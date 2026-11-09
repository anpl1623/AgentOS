import { useEffect, useMemo, useRef, useState } from "react";

import { ErrorBanner } from "../../components/common";
import { useCopy, useNow } from "../../components/hooks";
import type { EventView } from "../../bindings/EventView";
import { api } from "../../sdk/client";
import { reloadEventHistory, setFeedFilter, useEvents, useFeedFilter } from "../../sdk/eventStream";
import { usingFixtures } from "../../sdk/transport";
import { useAsync } from "../../sdk/useAsync";
import type { Navigate } from "../route";
import { ActivityHeader } from "./ActivityHeader";
import { ActivityStats } from "./ActivityStats";
import {
  DEEP_LOAD,
  NO_QUERY,
  SHALLOW_LOAD,
  canOpen,
  emptyMessage,
  feedNote,
  filterFeed,
  groupByDay,
  isFiltering,
  kindOptions,
  latestSequence,
  mergeFeed,
  toJsonl,
} from "./activityModel";
import { EmptyFeed } from "./EmptyFeed";
import { type EventFamily, familyCounts } from "./eventFamilies";
import { Feed } from "./Feed";
import { FeedFooter } from "./FeedFooter";
import { FeedToolbar } from "./FeedToolbar";
import { emptyAction, liveStatus } from "./feedSignals";
import { Record } from "./Record";
import { RunScope } from "./RunScope";
import { useArrivals, usePinOffset, useUnseen } from "./useFeedMotion";
import { useFollow } from "./useFollow";
import { useHush } from "./useHush";

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
  const [family, setFamily] = useState<EventFamily | "">("");
  const [openId, setOpenId] = useState<string | null>(null);
  const { copy, state: copyState } = useCopy();
  // Minute resolution is enough for "Today" to become "Yesterday" at midnight.
  const now = useNow();
  const today = new Date(now);

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
  const query = { text, kind, family, runId: runId ?? null };
  // Keyed on the query's fields: the object itself is new on every render.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const shown = useMemo(() => filterFeed(held, query), [held, text, kind, family, runId]);
  const groups = useMemo(() => groupByDay(shown), [shown]);
  const kinds = useMemo(() => kindOptions(held, kind), [held, kind]);
  // The chips count what every other filter keeps, so each says what pressing
  // it would show.
  const families = useMemo(
    () => familyCounts(filterFeed(held, { ...query, family: "" }), family),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [held, text, kind, family, runId],
  );
  // The figures read the whole stream, narrowed only to the run.
  const scoped = useMemo(
    () => filterFeed(stream.all, { ...NO_QUERY, runId: runId ?? null }),
    [stream.all, runId],
  );
  const unwritten = shown.filter((event) => !canOpen(event)).length;

  const failed = securityOnly ? history.error !== null : stream.error !== null;
  const loading = securityOnly ? history.loading : !stream.loaded && stream.error === null;
  const filtering = isFiltering(query);
  const note = feedNote({
    securityOnly,
    held: held.length,
    latest: latestSequence(held),
    requested: depth,
    received: securityOnly ? (history.data?.length ?? null) : null,
    read: stream.depth,
    runId: runId ?? null,
  });
  const live = liveStatus({
    fixtures: usingFixtures(),
    loaded: stream.loaded,
    failed: stream.error !== null,
  });

  const feedEnd = useFollow(follow, shown.at(-1)?.id);
  const arrived = useArrivals(held, securityOnly ? "security" : "all");
  const unseen = useUnseen(follow, shown);
  const root = useRef<HTMLDivElement>(null);
  usePinOffset(root);

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

  // Only a real change hushes: the release waits on the switch's effect,
  // which a choice that changes nothing would never run.
  const setSecurity = (next: boolean) => {
    if (next === securityOnly) return;
    hush();
    awaitingSecurity.current = true;
    setFeedFilter({ securityOnly: next });
  };

  // An explicit override: on re-engages and jumps, whatever the scroll
  // position last said.
  const setFollow = (next: boolean) => {
    setFeedFilter({ follow: next });
    if (next) feedEnd.current?.scrollIntoView({ block: "end" });
  };

  const clearFilters = () =>
    filterBy(() => {
      setText("");
      setKind("");
      setFamily("");
    });

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
  const securityButton = useRef<HTMLButtonElement>(null);

  // Each control below leaves the page as it acts — the tile stops being a
  // button once the security view is on, the jump pill once Follow is, an
  // empty state's button once rows arrive — and focus that is not moved
  // falls to the top of the document. It goes where the result is instead.
  // Without scrolling: each place is already in view, and a scroll here
  // would read to useFollow as the operator's own.
  const focusFeed = () => feed.current?.focus({ preventScroll: true });
  const showSecurity = () => {
    setSecurity(true);
    securityButton.current?.focus({ preventScroll: true });
  };
  const jumpToNewest = () => {
    setFollow(true);
    const rows = feed.current?.querySelectorAll<HTMLElement>("[data-event-id]");
    const newest = rows === undefined ? undefined : rows[rows.length - 1];
    (newest ?? feed.current)?.focus({ preventScroll: true });
  };

  const closeRecord = () => {
    const opener = openId;
    setOpenId(null);
    const row =
      opener === null
        ? null
        : feed.current?.querySelector<HTMLElement>(`[data-event-id="${CSS.escape(opener)}"]`);
    (row ?? feed.current)?.focus();
  };

  const emptyInput = { securityOnly, filtering, runId: runId ?? null, failed };
  const empty =
    shown.length === 0 && !loading ? (
      <EmptyFeed
        message={emptyMessage(emptyInput)}
        action={emptyAction(emptyInput)}
        onAction={{
          retry: () => {
            if (securityOnly) history.reload();
            else readAgain();
            focusFeed();
          },
          clear: () => {
            clearFilters();
            focusFeed();
          },
          // A navigation: the shell moves focus to the screen.
          "every-run": () => navigate({ name: "activity" }),
          "every-event": () => {
            setSecurity(false);
            focusFeed();
          },
          tasks: () => navigate({ name: "tasks" }),
        }}
      />
    ) : null;

  return (
    <div ref={root} className="activity">
      <ActivityHeader
        runScoped={Boolean(runId)}
        live={live}
        follow={follow}
        onFollow={setFollow}
        visible={shown.length}
        copyState={copyState}
        onCopy={() => void copy(toJsonl(shown))}
        loadBusy={deepening || history.refreshing || history.loading}
        onLoad={loadMore}
      />

      {runId ? (
        <RunScope
          runId={runId}
          count={held.filter((event) => event.run_id === runId).length}
          navigate={navigate}
        />
      ) : null}

      <ActivityStats
        events={scoped}
        loaded={stream.loaded}
        now={now}
        securityOnly={securityOnly}
        onSecurity={showSecurity}
      />

      <FeedToolbar
        text={text}
        onText={(next) => filterBy(() => setText(next))}
        kind={kind}
        kinds={kinds}
        onKind={(next) => filterBy(() => setKind(next))}
        family={family}
        families={families}
        onFamily={(next) => filterBy(() => setFamily(next))}
        securityOnly={securityOnly}
        onSecurity={setSecurity}
        securityRef={securityButton}
        filtering={filtering}
        onClear={clearFilters}
        shown={shown.length}
        held={held.length}
      />

      {history.error ? <ErrorBanner message={history.error} /> : null}
      {!securityOnly && stream.error ? <ErrorBanner message={stream.error} /> : null}

      <div className="activity-body">
        <div className={openId !== null ? "activity-split with-record" : "activity-split"}>
          <div className="activity-main">
            <Feed
              feedRef={feed}
              endRef={feedEnd}
              label={securityOnly ? "Security-relevant events" : "Activity"}
              speaking={follow && !hushed}
              loading={shown.length === 0 && loading}
              loadingWhat={securityOnly ? "security records" : "activity"}
              empty={empty}
              groups={groups}
              today={today}
              openId={openId}
              arrived={arrived}
              onToggle={(id) => setOpenId((current) => (current === id ? null : id))}
            />

            <FeedFooter
              unseen={unseen}
              onJump={jumpToNewest}
              note={note}
              unwritten={unwritten}
              onReadAgain={readAgain}
            />
          </div>

          {openId !== null ? (
            <Record id={openId} navigate={navigate} onClose={closeRecord} />
          ) : null}
        </div>
      </div>
    </div>
  );
}
