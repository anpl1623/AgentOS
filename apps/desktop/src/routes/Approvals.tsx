import { useCallback, useEffect, useState } from "react";

import { ErrorBanner, Loading, Risk } from "../components/common";
import type { ApprovalView } from "../bindings/ApprovalView";
import { api, describeError, events } from "../sdk/client";
import { ago } from "../sdk/format";
import { subscribe } from "../sdk/transport";
import { useAsync } from "../sdk/useAsync";

/**
 * Pending approvals.
 *
 * The screen the rest of the architecture exists to make meaningful: an agent
 * has asked to do something consequential, and a person decides. Everything a
 * decision needs is on the card, because an approval that sends someone hunting
 * through other screens is an approval that gets clicked without being read.
 */
export function Approvals({ onResolved }: { onResolved: () => void }) {
  const pending = useAsync(() => api.listPendingApprovals(), []);
  const { reload } = pending;

  // Requests appear while this screen is open, so it listens rather than only
  // polling — a card that takes five seconds to show up is five seconds an
  // agent spends blocked.
  useEffect(() => {
    const subscriptions = [
      subscribe(events.approvalRequested, reload),
      subscribe(events.approvalResolved, reload),
    ];
    return () => {
      for (const pendingUnsubscribe of subscriptions) {
        void pendingUnsubscribe.then((unsubscribe) => unsubscribe());
      }
    };
  }, [reload]);

  return (
    <>
      <div className="page-head">
        <h1>Approvals</h1>
      </div>
      <p className="page-sub">
        Actions an agent may not take without you. Nothing here has happened yet.
      </p>

      {pending.error ? <ErrorBanner message={pending.error} /> : null}
      {pending.loading && !pending.data ? <Loading what="approvals" /> : null}

      {pending.data?.length === 0 ? (
        <div className="panel">
          <div className="empty">
            Nothing is waiting on you.
            <div style={{ marginTop: 6 }} className="faint">
              Agents keep working; they stop here only for actions their policy routes to you.
            </div>
          </div>
        </div>
      ) : null}

      {pending.data?.map((approval) => (
        <ApprovalCard
          key={approval.id}
          approval={approval}
          onResolved={() => {
            reload();
            onResolved();
          }}
        />
      ))}
    </>
  );
}

function ApprovalCard({
  approval,
  onResolved,
}: {
  approval: ApprovalView;
  onResolved: () => void;
}) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showNote, setShowNote] = useState(false);

  const decide = useCallback(
    async (approved: boolean) => {
      setBusy(true);
      setError(null);
      try {
        const delivered = await api.resolveApproval({
          approval_id: approval.id,
          approved,
          note: note.trim() === "" ? null : note.trim(),
        });
        if (!delivered) {
          // The run finished or was cancelled while the card was on screen.
          setError(
            "That request is no longer waiting — the run has moved on. Nothing was changed.",
          );
        }
        onResolved();
      } catch (failure) {
        setError(describeError(failure));
      } finally {
        setBusy(false);
      }
    },
    [approval.id, note, onResolved],
  );

  return (
    <div className="approval">
      <div className="approval-head">
        <div>
          <div className="approval-title">
            {approval.agent_name} wants to run <span className="mono">{approval.tool}</span>
          </div>
          <div className="row-meta">
            <span>asked {ago(approval.requested_at)}</span>
          </div>
        </div>
        <Risk level={approval.risk} />
      </div>

      <div className="approval-body">
        <p className="approval-explanation">{approval.explanation}</p>

        {approval.tainted ? (
          <div className="approval-warning">
            This agent has read data from outside the trust boundary during this run. Whatever it
            is proposing may have been influenced by that content rather than by your objective.
            {approval.taint_sources.length > 0 ? (
              <div className="inline" style={{ marginTop: 8 }}>
                {approval.taint_sources.map((source) => (
                  <span key={source} className="tag">
                    {source}
                  </span>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}

        <dl className="facts">
          <dt>Working on</dt>
          <dd>{approval.objective}</dd>

          <dt>Because</dt>
          <dd>{approval.reason}</dd>

          {approval.affected_resources.length > 0 ? (
            <>
              <dt>Affects</dt>
              <dd>
                <div className="inline">
                  {approval.affected_resources.map((resource) => (
                    <span key={resource} className="tag">
                      {resource}
                    </span>
                  ))}
                </div>
              </dd>
            </>
          ) : null}
        </dl>

        <div className="field">
          <label htmlFor={`args-${approval.id}`}>Exact arguments</label>
          <pre className="code" id={`args-${approval.id}`}>
            {approval.arguments}
          </pre>
        </div>

        {showNote ? (
          <div className="field">
            <label htmlFor={`note-${approval.id}`}>
              Note (recorded in the audit log, and shown to the agent)
            </label>
            <input
              id={`note-${approval.id}`}
              value={note}
              onChange={(event) => setNote(event.target.value)}
              placeholder="Why are you declining?"
            />
          </div>
        ) : null}

        {error ? <ErrorBanner message={error} /> : null}
      </div>

      <div className="approval-foot">
        <button
          type="button"
          className="danger"
          disabled={busy}
          onClick={() => {
            if (!showNote) {
              setShowNote(true);
              return;
            }
            void decide(false);
          }}
        >
          {showNote ? "Confirm deny" : "Deny"}
        </button>
        {showNote ? (
          <button type="button" className="ghost" disabled={busy} onClick={() => setShowNote(false)}>
            Cancel
          </button>
        ) : null}
        <span className="spacer" />
        <button type="button" className="primary" disabled={busy} onClick={() => void decide(true)}>
          Approve
        </button>
      </div>
    </div>
  );
}
