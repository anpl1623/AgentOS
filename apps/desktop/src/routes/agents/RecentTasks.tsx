import type { TaskSummary } from "../../bindings/TaskSummary";
import { EmptyState, Icon, Row, Status } from "../../components/common";
import { ago } from "../../sdk/format";
import type { Navigate } from "../route";
import { Item, List } from "./lists";
import { Section } from "./Section";

/** The agent's latest tasks, newest first; each that has run opens its trace. */
export function RecentTasks({
  tasks,
  navigate,
}: {
  tasks: readonly TaskSummary[];
  navigate: Navigate;
}) {
  return (
    <Section
      id="agent-recent"
      title="Recent tasks"
      icon="tasks"
      aside={<span className="section-count">{tasks.length}</span>}
    >
      <div className="panel">
        {tasks.length === 0 ? (
          <EmptyState
            icon="tasks"
            title="Nothing yet"
            action={
              <button type="button" onClick={() => navigate({ name: "tasks" })}>
                Go to Tasks
              </button>
            }
          >
            Tasks given to this agent appear here, newest first.
          </EmptyState>
        ) : (
          <List labelledBy="agent-recent">
            {tasks.map((task) => (
              <Item key={task.id}>
                <RecentTask task={task} navigate={navigate} />
              </Item>
            ))}
          </List>
        )}
      </div>
    </Section>
  );
}

/** A recent task. One that never ran has no trace to open, so it is not a button. */
function RecentTask({ task, navigate }: { task: TaskSummary; navigate: Navigate }) {
  const run = task.latest_run;
  return (
    <Row onActivate={run ? () => navigate({ name: "tasks", runId: run.id }) : undefined}>
      <div className="row-main">
        <div className="row-title">{task.objective}</div>
        <div className="row-meta">
          <span>{ago(task.created_at)}</span>
          {run ? <span className="mono">{run.id}</span> : null}
        </div>
      </div>
      {run ? null : (
        <span className="verdict never">
          <span className="visually-hidden">task </span>Never ran
        </span>
      )}
      <Status status={task.status} />
      {run ? <Icon name="chevron-right" size={16} className="row-go" /> : null}
    </Row>
  );
}
