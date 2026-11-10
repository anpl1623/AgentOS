import { useCallback, useMemo, useRef } from "react";

import {
  EmptyState,
  ErrorBanner,
  Kbd,
  Loading,
  PageHeader,
  SkeletonRows,
} from "../../components/common";
import { useNow } from "../../components/hooks";
import { prefersReducedMotion } from "../../components/motion";
import { RECONCILE_EVERY_MS, usePendingApprovals } from "../../sdk/approvals";
import { cacheKey, useAsyncCached } from "../../sdk/cache";
import { api, events } from "../../sdk/client";
import { useLive, useVisibleInterval } from "../../sdk/live";
import { useNavigate } from "../../shell/router";
import { ApprovalCard } from "./ApprovalCard";
import { sortApprovals } from "./approvalsModel";
import { deskEntries } from "./decisionModel";
import { DecisionLog } from "./DecisionLog";
import { DepartingCard } from "./DepartingCard";
import { queueSummary, recentTally } from "./deskModel";
import { QueueRail } from "./QueueRail";
import { QueueStats, WaitingPill } from "./QueueStats";
import { RecentlyDecided } from "./RecentlyDecided";
import { useDecisions } from "./useDecisions";
import { useDesk } from "./useDesk";

/** How many answered requests the Recently decided panel lists. */
const RECENT_LIMIT = 20;

/** The cards column, which the side-by-side queue presents into. */
const CARDS_ID = "approvals-cards";

/**
 * Pending approvals.
 *
 * The screen the rest of the architecture exists to make meaningful: an agent
 * has asked to do something consequential, and a person decides. On a wide
 * window it is a desk: the queue down the side, most dangerous first, and the
 * request being read beside it, held in view while the queue scrolls. Narrower,
 * the cards stack and are the queue. Which of the two is drawn is the
 * stylesheet's choice, so the first paint is right at any width.
 *
 * The composition only. The selection and its keyboard are `useDesk`; making
 * a decision is `useDecisions`; the rules both follow are in the models.
 */
export function Approvals({
  focus,
}: {
  /** One pending approval to select and bring into view, from a link. */
  focus?: string | undefined;
}) {
  const navigate = useNavigate();
  const { approvals, loaded, error, failures } = usePendingApprovals();
  const sorted = useMemo(() => sortApprovals(approvals), [approvals]);
  // Minute resolution, for "blocked for" and the receipts' ages.
  const now = useNow();

  const recent = useAsyncCached(cacheKey("list_recent_approvals", RECENT_LIMIT), () =>
    api.listRecentApprovals(RECENT_LIMIT),
  );
  const reloadRecent = recent.reload;
  useLive(events.approvalResolved, reloadRecent);
  useVisibleInterval(reloadRecent, RECONCILE_EVERY_MS);

  const desk = useDesk(sorted, focus, loaded);
  const { dispatch, selection, shown } = desk;
  const decisions = useDecisions(dispatch, reloadRecent, desk.indexOf);
  const entries = useMemo(
    () => deskEntries(sorted, decisions.departures),
    [sorted, decisions.departures],
  );
  const summary = useMemo(() => queueSummary(sorted, now), [sorted, now]);
  const tally = useMemo(() => recentTally(recent.data ?? []), [recent.data]);

  const recentHeading = useRef<HTMLHeadingElement>(null);
  const toRecent = useCallback(() => {
    const heading = recentHeading.current;
    heading?.focus({ preventScroll: true });
    heading?.scrollIntoView({
      block: "start",
      behavior: prefersReducedMotion() ? "auto" : "smooth",
    });
  }, []);

  return (
    <div className="approvals-screen">
      <PageHeader
        eyebrow="Oversight"
        title="Approvals"
        subtitle="Actions an agent may not take without you. Nothing here has happened yet."
        actions={<WaitingPill waiting={summary.waiting} loaded={loaded} />}
      />

      {error !== null && !loaded ? <ErrorBanner message={error} /> : null}
      {error !== null && loaded ? (
        <div className="banner warn">
          The runtime did not answer the last check, so this list may be out of date.
        </div>
      ) : null}
      {desk.focusMissing ? (
        <div className="banner info">
          The request you followed is no longer waiting. If it was decided, it is under Recently
          decided.
        </div>
      ) : null}

      <QueueStats
        summary={summary}
        loaded={loaded}
        recent={recent.data?.length ?? null}
        tally={tally}
        onRecent={toRecent}
      />

      <DecisionLog
        status={decisions.status}
        tone={decisions.statusTone}
        statusRef={decisions.statusRef}
        receipts={decisions.receipts}
      />

      {!loaded && error === null ? (
        <div className="panel">
          <Loading what="approvals" />
          <SkeletonRows count={3} />
        </div>
      ) : null}

      {loaded && entries.length === 0 ? (
        <div className="panel approvals-empty">
          <EmptyState
            icon="shield"
            title="Nothing needs you"
            action={
              <button type="button" onClick={() => navigate({ name: "tasks" })}>
                See what agents are doing
              </button>
            }
          >
            Agents keep working; they stop here only for actions their policy routes to you.
          </EmptyState>
        </div>
      ) : null}

      <p className="visually-hidden" role="status">
        {desk.arrival}
      </p>

      {entries.length > 0 ? (
        <>
          <p className="field-note approvals-keys">
            <Kbd>j</Kbd> <Kbd>k</Kbd> or <Kbd>↓</Kbd> <Kbd>↑</Kbd> move between requests, most
            dangerous first. <Kbd>Esc</Kbd> sets a half-made decision down. No key approves or
            denies; the buttons do, and each asks you to confirm.
          </p>
          <div className="approvals-desk">
            <QueueRail
              entries={entries}
              waiting={sorted.length}
              shown={shown}
              intent={selection.intent}
              failures={failures}
              now={now}
              cardsId={CARDS_ID}
              registerItem={desk.registerItem}
              onSelect={(id) => dispatch({ kind: "select", id })}
            />
            <div className="approvals-cards" id={CARDS_ID} role="list" aria-label="Waiting on you">
              {entries.map((entry) => {
                if (entry.kind === "ghost") {
                  const { departure } = entry;
                  return (
                    <DepartingCard
                      key={`leaving-${departure.approval.id}`}
                      departure={departure}
                      now={now}
                    />
                  );
                }
                const { approval } = entry;
                const isShown = shown === approval.id;
                return (
                  <div
                    role="listitem"
                    key={approval.id}
                    className={isShown ? "approval-slot shown" : "approval-slot"}
                  >
                    <ApprovalCard
                      approval={approval}
                      now={now}
                      intent={selection.intent?.id === approval.id ? selection.intent : null}
                      shown={isShown}
                      selected={selection.selected === approval.id}
                      linked={focus === approval.id}
                      failure={failures.get(approval.id) ?? null}
                      register={desk.register}
                      onSelect={() => dispatch({ kind: "select", id: approval.id })}
                      onIntend={(kind) =>
                        dispatch({ kind: "intend", id: approval.id, intent: kind })
                      }
                      onNote={(text) => dispatch({ kind: "note", id: approval.id, text })}
                      onCancel={() => dispatch({ kind: "cancel" })}
                      onConfirm={(intent) => void decisions.decide(approval, intent)}
                      onDrill={() => navigate({ name: "tasks", runId: approval.run_id })}
                    />
                  </div>
                );
              })}
            </div>
          </div>
        </>
      ) : null}

      <RecentlyDecided
        recent={recent}
        headingRef={recentHeading}
        onOpen={(runId) => navigate({ name: "tasks", runId })}
      />
    </div>
  );
}
