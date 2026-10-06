import {
  type MouseEvent,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import type { ApprovalView } from "../bindings/ApprovalView";
import { ArgumentBlock, SCROLL_REGION, Visible } from "../components/ArgumentBlock";
import { Empty, ErrorBanner, Loading, Risk, Row } from "../components/common";
import { useNow } from "../components/hooks";
import {
  MOVED_ON,
  RECONCILE_EVERY_MS,
  resolveApproval,
  usePendingApprovals,
} from "../sdk/approvals";
import { cacheKey, useAsyncCached } from "../sdk/cache";
import { api, describeError, events } from "../sdk/client";
import { ago, humanise } from "../sdk/format";
import { useLive, useVisibleInterval } from "../sdk/live";
import { isTypingTarget } from "../shell/keys";
import { modalOpen } from "../shell/modal";
import { useNavigate } from "../shell/router";
import {
  type Intent,
  type IntentKind,
  arrivals,
  askedOnlyForTaint,
  blockedFor,
  budgetLine,
  decisionTone,
  describedWarning,
  idleSelection,
  isDeliberate,
  nextSelection,
  offersStop,
  sortApprovals,
} from "./approvalsModel";

/** How many answered requests the Recently decided panel lists. */
const RECENT_LIMIT = 20;

/** The selection keys. Movement only; see the keyboard effect for why. */
const MOVES: Readonly<Record<string, 1 | -1>> = { j: 1, ArrowDown: 1, k: -1, ArrowUp: -1 };

/** A decision made while this screen was open, for the receipt above the queue. */
interface Receipt {
  id: string;
  kind: IntentKind;
  tool: string;
  agent: string;
  at: string;
  /** For a denial that also stopped the run: what became of the run. */
  run?: RunEnd["kind"];
}

/** What became of a run the operator asked to stop. */
type RunEnd = { kind: "stopped" } | { kind: "ended" } | { kind: "failed"; message: string };

const RUN_SENTENCE: Record<RunEnd["kind"], string> = {
  stopped: "The run was stopped.",
  ended: "The run had already ended.",
  failed: "The run could not be stopped:",
};

const RUN_CLAUSE: Record<RunEnd["kind"], string> = {
  stopped: " and stopped the run",
  ended: " (the run had already ended)",
  failed: " (the run could not be stopped)",
};

const DONE: Record<IntentKind, string> = {
  approve: "Approved",
  deny: "Denied",
  stop: "Denied",
};

const DOING: Record<IntentKind, string> = {
  approve: "Approving",
  deny: "Denying",
  stop: "Denying",
};

/**
 * Pending approvals.
 *
 * The screen the rest of the architecture exists to make meaningful: an agent
 * has asked to do something consequential, and a person decides. Everything a
 * decision needs is on the card, because an approval that sends someone hunting
 * through other screens is an approval that gets clicked without being read.
 *
 * The queue is the shared store's, so a decision is optimistic: the card leaves
 * and the badge drops on the confirm, before the runtime answers. What was
 * decided is then said in the page's status region, which takes focus, and
 * kept in a receipt above the queue for as long as the screen is open.
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
  const ids = useMemo(() => sorted.map((approval) => approval.id), [sorted]);
  const [selection, dispatch] = useReducer(nextSelection, idleSelection);
  const [status, setStatus] = useState("");
  const [receipts, setReceipts] = useState<readonly Receipt[]>([]);
  const [focusMissing, setFocusMissing] = useState(false);
  const statusRef = useRef<HTMLDivElement>(null);
  const cards = useRef(new Map<string, HTMLElement>());
  const order = useRef(ids);
  order.current = ids;
  const current = useRef(selection);
  current.current = selection;
  // Minute resolution, for "blocked for" and the receipts' ages.
  const now = useNow();

  const recent = useAsyncCached(cacheKey("list_recent_approvals", RECENT_LIMIT), () =>
    api.listRecentApprovals(RECENT_LIMIT),
  );
  const reloadRecent = recent.reload;
  useLive(events.approvalResolved, reloadRecent);
  useVisibleInterval(reloadRecent, RECONCILE_EVERY_MS);

  // A card that leaves the queue takes its selection and its intent with it.
  useEffect(() => dispatch({ kind: "queue", ids }), [ids]);

  // New requests are announced in one line each time the queue grows; what
  // is in the queue when it first loads is not announced at all.
  const seen = useRef<ReadonlySet<string> | null>(null);
  const [arrival, setArrival] = useState("");
  useEffect(() => {
    if (!loaded) return;
    const next = arrivals(seen.current, sorted);
    seen.current = next.seen;
    if (next.notice !== null) setArrival(next.notice);
  }, [loaded, sorted]);

  // A link names one card: select it and bring it into view once, when the
  // queue has loaded. Focus stays where the shell puts it on arrival.
  const landed = useRef<string | null>(null);
  useEffect(() => {
    if (focus === undefined || !loaded || landed.current === focus) return;
    landed.current = focus;
    const card = cards.current.get(focus);
    setFocusMissing(card === undefined);
    if (card === undefined) return;
    dispatch({ kind: "select", id: focus });
    card.scrollIntoView({ block: "nearest" });
  }, [focus, loaded, ids]);

  // The keyboard moves through the queue and can set a half-made decision
  // down. It cannot make one. There is deliberately no key for approve, deny
  // or confirm: the queue reflows the moment a decision is confirmed, so a
  // repeated or queued keystroke lands on whichever card slid into the
  // decided one's place, and that is how an operator approves a request they
  // never read. Approve and Deny are ordinary buttons, reached with Tab and
  // pressed as buttons are, and each still takes a second, confirming press.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || modalOpen()) return;
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.key === "Escape") {
        // Honoured from the note field too: it is the way out of the field.
        if (current.current.intent === null) return;
        event.preventDefault();
        dispatch({ kind: "cancel" });
        return;
      }
      if (isTypingTarget(event.target)) return;
      const by = MOVES[event.key];
      if (by === undefined) return;
      const arrow = event.key === "ArrowDown" || event.key === "ArrowUp";
      if (
        arrow &&
        event.target instanceof Element &&
        event.target.closest(SCROLL_REGION) !== null
      ) {
        return;
      }
      event.preventDefault();
      // A move puts the keyboard on the card it selected, so Tab continues
      // into that card rather than from wherever focus last was.
      const move = { kind: "move", by, order: order.current } as const;
      const target = nextSelection(current.current, move).selected;
      dispatch(move);
      const card = target === null ? undefined : cards.current.get(target);
      card?.focus({ preventScroll: true });
      card?.scrollIntoView({ block: "nearest" });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const register = useCallback((id: string, element: HTMLElement | null) => {
    if (element === null) cards.current.delete(id);
    else cards.current.set(id, element);
  }, []);

  const decide = useCallback(
    async (approval: ApprovalView, intent: Intent) => {
      // Nothing stays armed and nothing stays selected: the card that takes
      // this one's place has not been read. Focus goes to the status line,
      // never to the next card's buttons, and it goes there before the card
      // leaves, so it is not dropped on the page body.
      dispatch({ kind: "resolved" });
      const what = `${approval.tool} for ${approval.agent_name}`;
      setStatus(
        `${DOING[intent.kind]} ${what}${intent.kind === "stop" ? " and stopping the run" : ""}…`,
      );
      statusRef.current?.focus();

      const note = intent.note.trim();
      const outcome = await resolveApproval({
        approval_id: approval.id,
        approved: intent.kind === "approve",
        note: note === "" ? null : note,
      });
      if (outcome.kind === "failed") {
        setStatus(
          `Could not answer: ${outcome.message} The request is back in the list; nothing was changed.`,
        );
        return;
      }

      const said = outcome.kind === "delivered" ? `${DONE[intent.kind]} ${what}.` : MOVED_ON;
      let run: RunEnd | undefined;
      if (intent.kind === "stop") {
        // Stopped even when the request had moved on: the operator asked for
        // the run to end, and a request that has gone says nothing about it.
        try {
          run = (await api.cancelRun(approval.run_id)) ? { kind: "stopped" } : { kind: "ended" };
        } catch (failure) {
          run = { kind: "failed", message: describeError(failure) };
        }
      }
      setStatus(
        run === undefined
          ? said
          : `${said} ${RUN_SENTENCE[run.kind]}${run.kind === "failed" ? ` ${run.message}` : ""}`,
      );
      if (outcome.kind === "delivered") {
        setReceipts((list) => [
          {
            id: approval.id,
            kind: intent.kind,
            tool: approval.tool,
            agent: approval.agent_name,
            at: new Date().toISOString(),
            ...(run === undefined ? {} : { run: run.kind }),
          },
          ...list,
        ]);
      }
      reloadRecent();
    },
    [reloadRecent],
  );

  return (
    <>
      <div className="page-head">
        <h1>Approvals</h1>
      </div>
      <p className="page-sub">
        Actions an agent may not take without you. Nothing here has happened yet.
      </p>
      <p className="field-note">
        <kbd>j</kbd> <kbd>k</kbd> or <kbd>↓</kbd> <kbd>↑</kbd> move between requests, most dangerous
        first. <kbd>Esc</kbd> sets a half-made decision down. No key approves or denies; the buttons
        do, and each asks you to confirm.
      </p>

      {error !== null && !loaded ? <ErrorBanner message={error} /> : null}
      {error !== null && loaded ? (
        <div className="banner warn">
          The runtime did not answer the last check, so this list may be out of date.
        </div>
      ) : null}
      {focusMissing ? (
        <div className="banner info">
          The request you followed is no longer waiting. If it was decided, it is under Recently
          decided.
        </div>
      ) : null}

      <div className="field">
        <div className="decided" role="status" tabIndex={-1} ref={statusRef}>
          {status}
        </div>
        {receipts.length > 0 ? (
          <div role="list" aria-label="Decided while this screen was open">
            {receipts.map((receipt) => (
              <div role="listitem" className="decided" key={receipt.id}>
                {DONE[receipt.kind]} <span className="mono">{receipt.tool}</span> for{" "}
                {receipt.agent}
                {receipt.run === undefined ? null : RUN_CLAUSE[receipt.run]}
                {" · "}
                {ago(receipt.at)}
              </div>
            ))}
          </div>
        ) : null}
      </div>

      {!loaded && error === null ? <Loading what="approvals" /> : null}

      {loaded && sorted.length === 0 ? (
        <div className="panel">
          <Empty>
            Nothing is waiting on you.
            <div className="field-note">
              Agents keep working; they stop here only for actions their policy routes to you.
            </div>
          </Empty>
        </div>
      ) : null}

      <p className="visually-hidden" role="status">
        {arrival}
      </p>
      <div className="stack" role="list" aria-label="Waiting on you">
        {sorted.map((approval) => (
          <div role="listitem" key={approval.id}>
            <ApprovalCard
              approval={approval}
              now={now}
              intent={selection.intent?.id === approval.id ? selection.intent : null}
              selected={selection.selected === approval.id}
              linked={focus === approval.id}
              failure={failures.get(approval.id) ?? null}
              register={register}
              onSelect={() => dispatch({ kind: "select", id: approval.id })}
              onIntend={(kind) => dispatch({ kind: "intend", id: approval.id, intent: kind })}
              onNote={(text) => dispatch({ kind: "note", id: approval.id, text })}
              onCancel={() => dispatch({ kind: "cancel" })}
              onConfirm={(intent) => void decide(approval, intent)}
              onDrill={() => navigate({ name: "tasks", runId: approval.run_id })}
            />
          </div>
        ))}
      </div>

      <h2>Recently decided</h2>
      <div className="panel">
        {recent.error !== null && recent.data === null ? (
          <ErrorBanner message={recent.error} />
        ) : null}
        {recent.loading ? <Loading what="recent decisions" /> : null}
        {recent.data?.length === 0 ? <Empty>Nothing has been decided yet.</Empty> : null}
        {recent.data?.map((approval) => (
          <Row
            key={approval.id}
            onActivate={() => navigate({ name: "tasks", runId: approval.run_id })}
          >
            <div className="row-main">
              <div className="row-title">
                <span className="mono">{approval.tool}</span> for {approval.agent_name}
              </div>
              <div className="row-meta">
                <span>{approval.decided_at === null ? "undated" : ago(approval.decided_at)}</span>
                {approval.note === null ? null : <span>“{approval.note}”</span>}
              </div>
            </div>
            <span className={`verdict ${decisionTone(approval.status)}`}>
              <span className="visually-hidden">decision </span>
              {sentence(approval.status)}
            </span>
          </Row>
        ))}
      </div>
      <p className="field-note">A decision cannot be withdrawn once the run has been told.</p>
    </>
  );
}

function sentence(value: string): string {
  const words = humanise(value);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** A click that counts, for the decision buttons; see `isDeliberate`. */
function deliberate(event: MouseEvent): boolean {
  return isDeliberate(event.detail);
}

function ApprovalCard({
  approval,
  now,
  intent,
  selected,
  linked,
  failure,
  register,
  onSelect,
  onIntend,
  onNote,
  onCancel,
  onConfirm,
  onDrill,
}: {
  approval: ApprovalView;
  now: number;
  /** The half-made decision, when it is this card's. */
  intent: Intent | null;
  selected: boolean;
  /** The card a link sent the operator to. */
  linked: boolean;
  /** Why the last answer to this card failed, if it did. */
  failure: string | null;
  register: (id: string, element: HTMLElement | null) => void;
  onSelect: () => void;
  onIntend: (kind: IntentKind) => void;
  onNote: (text: string) => void;
  onCancel: () => void;
  onConfirm: (intent: Intent) => void;
  onDrill: () => void;
}) {
  const article = useRef<HTMLElement>(null);
  const note = useRef<HTMLInputElement>(null);
  const buttons = useRef(new Map<IntentKind, HTMLButtonElement>());
  const { id } = approval;
  const titleId = `approval-${id}-title`;
  const taintId = `approval-${id}-taint`;
  const noteId = `approval-${id}-note`;
  const stop = offersStop(approval);
  const budget = budgetLine(approval.asked_this_run, approval.approval_budget);
  const described = describedWarning(approval);
  const kind = intent?.kind ?? null;

  useEffect(() => {
    register(id, article.current);
    return () => register(id, null);
  }, [id, register]);

  // Arming a decision opens the note and puts the keyboard in it. A second
  // press of the same key therefore types into the note; it does not confirm.
  // Setting the decision down puts the keyboard back on the button that armed
  // it, unless focus has already gone somewhere on purpose.
  const armed = useRef<IntentKind | null>(null);
  useEffect(() => {
    const was = armed.current;
    armed.current = kind;
    if (kind !== null) {
      note.current?.focus();
      return;
    }
    const active = document.activeElement;
    if (was !== null && (active === null || active === document.body)) {
      buttons.current.get(was)?.focus();
    }
  }, [kind]);

  const keep = (which: IntentKind) => (element: HTMLButtonElement | null) => {
    if (element === null) buttons.current.delete(which);
    else buttons.current.set(which, element);
  };

  const className = ["approval", selected ? "selected" : "", selected && linked ? "focused" : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <article
      ref={article}
      className={className}
      aria-labelledby={titleId}
      aria-describedby={approval.tainted ? taintId : undefined}
      tabIndex={-1}
      onFocus={onSelect}
    >
      <div className="approval-head">
        <div>
          <h2 id={titleId} className="approval-title">
            {approval.agent_name} wants to run <span className="mono">{approval.tool}</span>
          </h2>
          <div className="row-meta">
            <span>blocked for {blockedFor(approval.requested_at, now)}</span>
            {budget === null ? null : <span>{budget}</span>}
          </div>
        </div>
        <div className="inline">
          <button type="button" className="ghost" onClick={onDrill}>
            What it has done so far →
          </button>
          <Risk level={approval.risk} />
        </div>
      </div>

      {/* Bounded and scrollable, so it takes a tab stop: a keyboard user must
          be able to scroll to everything they are being asked to allow. */}
      <div
        className="approval-body"
        tabIndex={0}
        role="group"
        aria-label="What it is asking to do"
        data-scroll-region=""
      >
        {described !== null ? (
          <div className="banner warn" role="note">
            {described}
          </div>
        ) : null}
        <p className="approval-explanation">
          <Visible text={approval.explanation} />
        </p>

        {approval.tainted ? <TaintNote id={taintId} approval={approval} /> : null}

        <dl className="facts">
          <dt>Working on</dt>
          <dd>
            <Visible text={approval.objective} />
          </dd>

          <dt>Because</dt>
          <dd>
            <Visible text={approval.reason} />
          </dd>

          {approval.affected_resources.length > 0 ? (
            <>
              <dt>Affects</dt>
              <dd>
                <div className="inline" role="list" aria-label="Resources it affects">
                  {approval.affected_resources.map((resource) => (
                    <span key={resource} className="tag" role="listitem">
                      <Visible text={resource} />
                    </span>
                  ))}
                </div>
              </dd>
            </>
          ) : null}
        </dl>

        <ArgumentBlock json={approval.arguments} id={`approval-${id}-args`} />

        {intent !== null ? (
          <div className="field">
            <label htmlFor={noteId}>
              {intent.kind === "approve" ? "Why are you allowing this?" : "Why are you declining?"}
            </label>
            <input
              id={noteId}
              ref={note}
              value={intent.note}
              maxLength={2000}
              onChange={(event) => onNote(event.target.value)}
            />
            <p className="field-note">
              {intent.kind === "approve"
                ? "Optional. Kept with your decision in the audit log."
                : "Optional. Kept with your decision in the audit log, and given to the agent as the reason."}
            </p>
          </div>
        ) : null}

        {failure !== null ? <ErrorBanner message={failure} /> : null}

        {stop ? (
          <p className="faint">
            Refusing one call leaves the run reasoning from the same context that produced it. Deny
            and stop the run ends it as well.
          </p>
        ) : null}
      </div>

      {/* Deny on the left and Approve past the spacer, as everywhere a
          deny-by-default product asks. Both take two presses: an interface
          must not make allowing an action cheaper than refusing it. */}
      <div className="approval-foot">
        {intent === null ? (
          <>
            <button
              type="button"
              className="danger"
              ref={keep("deny")}
              onClick={(event) => deliberate(event) && onIntend("deny")}
            >
              Deny
            </button>
            {stop ? (
              <button
                type="button"
                className="danger"
                ref={keep("stop")}
                onClick={(event) => deliberate(event) && onIntend("stop")}
              >
                Deny and stop the run
              </button>
            ) : null}
            <span className="spacer" />
            <button
              type="button"
              className="primary"
              ref={keep("approve")}
              onClick={(event) => deliberate(event) && onIntend("approve")}
            >
              Approve
            </button>
          </>
        ) : intent.kind === "approve" ? (
          <>
            <span className="spacer" />
            <button type="button" className="ghost" onClick={onCancel}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              onClick={(event) => deliberate(event) && onConfirm(intent)}
            >
              Confirm approve
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="danger"
              onClick={(event) => deliberate(event) && onConfirm(intent)}
            >
              {intent.kind === "stop" ? "Confirm deny and stop" : "Confirm deny"}
            </button>
            <button type="button" className="ghost" onClick={onCancel}>
              Cancel
            </button>
            <span className="spacer" />
          </>
        )}
      </div>
    </article>
  );
}

/**
 * Why a run that has read untrusted data is being asked.
 *
 * A note the article names in `aria-describedby`, so the warning is read
 * before the decision buttons are reached rather than after, and above the
 * facts it casts doubt on.
 */
function TaintNote({ id, approval }: { id: string; approval: ApprovalView }) {
  const sources = approval.taint_sources;
  const onlyTaint = askedOnlyForTaint(approval);
  return (
    <div className="approval-warning" role="note" id={id}>
      {onlyTaint
        ? sources.length > 0
          ? "This would have run without asking. It is being asked because this run read:"
          : "This would have run without asking. It is being asked because this run has read data from outside the trust boundary."
        : "This agent has read data from outside the trust boundary during this run. Whatever it is proposing may have been influenced by that content rather than by your objective."}
      {sources.length > 0 ? (
        <div className="inline reveal" role="list" aria-label="Untrusted sources it read">
          {sources.map((source) => (
            <span key={source} className="tag" role="listitem">
              <Visible text={source} />
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}
