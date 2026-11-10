import { CopyButton, Icon } from "../../components/common";
import type { Navigate } from "../route";

/**
 * The band that says the feed is narrowed to one run, from the address.
 *
 * It names the run, offers the run's own trace (which has the whole run,
 * where this feed has only what the window holds), and widens back to every
 * run. Widening is a navigation, so Back returns to the narrowed view.
 */
export function RunScope({
  runId,
  count,
  navigate,
}: {
  runId: string;
  /** This run's events among those held. */
  count: number;
  navigate: Navigate;
}) {
  return (
    <section className="run-scope" aria-labelledby="run-scope-title">
      <span className="run-scope-mark" aria-hidden="true">
        <Icon name="tasks" size={18} />
      </span>
      <div className="run-scope-text">
        <p id="run-scope-title" className="run-scope-title">
          Showing one run
        </p>
        <p className="run-scope-id">
          <span className="mono" title={runId}>
            {runId}
          </span>
          <CopyButton text={runId} label="Copy the run id" />
          <span className="run-scope-count">
            {count === 1 ? "1 event held" : `${count} events held`}
          </span>
        </p>
      </div>
      <div className="run-scope-actions">
        <button type="button" onClick={() => navigate({ name: "tasks", runId })}>
          Open run
          <Icon name="chevron-right" size={16} />
        </button>
        <button
          type="button"
          className="ghost"
          aria-label="Show every run"
          onClick={() => navigate({ name: "activity" })}
        >
          <Icon name="x" size={16} />
          Every run
        </button>
      </div>
    </section>
  );
}
