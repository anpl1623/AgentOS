import { usePop } from "../components/motion";
import { usePendingApprovals } from "../sdk/approvals";

/**
 * The count of approvals waiting, read from the shared queue so the badge and
 * the cards cannot disagree. Drawn on the Approvals nav item and on the top
 * bar's approvals button.
 *
 * The count pops when it changes, never when it is first drawn: a new request
 * arriving while the operator is elsewhere should catch the eye once.
 *
 * When the runtime cannot be read the count is kept and marked stale: dropping
 * it to nothing would tell the operator nothing is waiting on them, which is
 * the wrong failure here. It is announced by {@link ApprovalsAnnouncer}, not
 * here: this sits inside a button, whose content is presentational, and a live
 * region there is not reliably spoken.
 */
export function ApprovalsCount() {
  const { approvals, error } = usePendingApprovals();
  const count = approvals.length;
  const { key, popping } = usePop(count);
  return (
    <span className="nav-meta">
      {count > 0 ? (
        <span key={key} className={popping ? "nav-count pop" : "nav-count"}>
          {count}
          <span className="visually-hidden"> waiting on you</span>
        </span>
      ) : null}
      {error !== null ? (
        <span className="stale">
          stale
          <span className="visually-hidden">: the runtime did not answer the last check</span>
        </span>
      ) : null}
    </span>
  );
}

/**
 * Speaks the approvals count when it changes, so a request raised while the
 * operator is on another screen is heard, not only drawn.
 *
 * Always present, because a region that appears with its first number is not
 * announced. It sits outside the nav button, for the reason
 * {@link ApprovalsCount} gives, and outside the sidebar too: the narrow layout
 * hides the sidebar, and nothing is spoken from a hidden region.
 */
export function ApprovalsAnnouncer() {
  const count = usePendingApprovals().approvals.length;
  return (
    <span className="visually-hidden" aria-live="polite">
      {count === 0 ? "" : count === 1 ? "1 approval waiting on you" : `${count} approvals waiting on you`}
    </span>
  );
}
