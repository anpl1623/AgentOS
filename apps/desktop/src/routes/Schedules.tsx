import { PageHeader } from "../components/common";

/**
 * Schedules, not yet reachable from the window.
 *
 * A placeholder so the sidebar can list the screen a wave before it exists;
 * the screen that lists, pauses and resumes schedules replaces this file.
 */
export function Schedules() {
  return (
    <>
      <PageHeader
        title="Schedules"
        subtitle="Standing instructions: an objective an agent is given on a cron, on an interval, or once."
      />
      <div className="panel">
        <div className="panel-body">
          <p>
            The runtime keeps schedules, and this screen reaches them next. Until it does,{" "}
            <code className="mono">agentos schedule list</code> shows them and{" "}
            <code className="mono">agentos schedule pause &lt;name&gt;</code> stops one firing.
          </p>
          <p className="field-note">
            Nobody watches a scheduled run, so every approval it would ask for is refused with a
            note the agent can read and plan around.
          </p>
        </div>
      </div>
    </>
  );
}
