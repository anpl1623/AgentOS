import type { CSSProperties } from "react";

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { TaskSummary } from "../../bindings/TaskSummary";
import { EmptyState, LiveDot, SkeletonRows, State, Status, Tainted } from "../../components/common";
import { useNow } from "../../components/hooks";
import { ago, tokens } from "../../sdk/format";
import type { Navigate } from "../route";
import { runClock, runMotion, stepBudget } from "./overviewModel";
import { Item, List, Section, SeeAll } from "./Section";

/** Placeholder rows on first paint. */
const SKELETON_ROWS = 3;

/**
 * The live clock's tick. Only the clocks themselves re-render each second: a
 * running clock is the one place on the screen where a second passing is
 * news, and with nothing running there is no clock and nothing ticks.
 */
const CLOCK_MS = 1_000;

/**
 * What the agents are doing right now, one card per task.
 *
 * Each card carries a dot that pulses while the run is moving, a clock that
 * counts up from when it started, and a bar of how much of its agent's step
 * limit it has spent, with a band of light crossing it while it fills. A run
 * paused on an approval holds still in the attention hue.
 */
export function Running({
  tasks,
  agents,
  first,
  navigate,
}: {
  /** `null` before the first answer, or after a failed one. */
  tasks: readonly TaskSummary[] | null;
  agents: readonly AgentSummary[];
  first: boolean;
  navigate: Navigate;
}) {
  return (
    <Section
      id="dashboard-running"
      title="Running"
      icon="play"
      count={tasks ? tasks.length : null}
      tail={<SeeAll label="Tasks" onActivate={() => navigate({ name: "tasks" })} />}
    >
      {first ? (
        <div className="panel">
          <SkeletonRows count={SKELETON_ROWS} />
        </div>
      ) : !tasks ? null : tasks.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon="play"
            title="No agent is working right now."
            action={
              <button type="button" onClick={() => navigate({ name: "tasks" })}>
                Give an agent a task
              </button>
            }
          />
        </div>
      ) : (
        <List labelledBy="dashboard-running" className="dash-runs stagger">
          {tasks.map((task) => (
            <Item key={task.id}>
              <RunCard
                task={task}
                maxSteps={agents.find((agent) => agent.id === task.agent_id)?.max_steps}
                navigate={navigate}
              />
            </Item>
          ))}
        </List>
      )}
    </Section>
  );
}

function RunCard({
  task,
  maxSteps,
  navigate,
}: {
  task: TaskSummary;
  maxSteps: number | undefined;
  navigate: Navigate;
}) {
  const run = task.latest_run;
  const motion = run ? runMotion(run.state) : { tone: "info" as const, moving: false };
  const budget = run ? stepBudget(run.steps, maxSteps) : null;

  const body = (
    <>
      <span className="dash-run-head">
        <LiveDot tone={motion.tone} pulse={motion.moving} />
        <span className="dash-run-title">{task.objective}</span>
        {run ? <State state={run.state} /> : <Status status={task.status} />}
      </span>
      <span className="dash-run-meta">
        <span className="dash-run-agent">{task.agent_name}</span>
        {run ? (
          <>
            {/* The steps are on the budget bar's label when there is one. */}
            <span>
              {budget ? null : `${run.steps} steps · `}
              {tokens(run.input_tokens + run.output_tokens)} tokens
            </span>
            {run.tainted ? <Tainted /> : null}
            <RunClock startedAt={run.started_at} />
          </>
        ) : (
          <span>queued {ago(task.created_at)}</span>
        )}
      </span>
      {budget ? (
        <span className="dash-run-budget">
          <span className="dash-meter" aria-hidden="true">
            <span
              className={motion.moving ? "dash-meter-fill sheen" : "dash-meter-fill"}
              style={{ "--w": `${budget.percent}%` } as CSSProperties}
            />
          </span>
          <span className="dash-run-budget-label">{budget.label}</span>
        </span>
      ) : null}
    </>
  );

  // A queued task has no run to open; announced as a button, it would take a
  // tab stop and then ignore the key press.
  if (!run) return <div className="dash-run">{body}</div>;
  return (
    <button
      type="button"
      className="dash-run lift"
      data-spotlight
      onClick={() => navigate({ name: "tasks", runId: run.id })}
    >
      {body}
    </button>
  );
}

/**
 * How long a run has been going, to the second. The one part of a card that
 * ticks, so a second passing redraws these few characters and nothing else.
 * The words for assistive technology are part of the card's name, not a live
 * region: a clock read aloud every second would drown out everything else.
 */
function RunClock({ startedAt }: { startedAt: string }) {
  const now = useNow(CLOCK_MS);
  return (
    <span className="dash-run-clock">
      <span className="visually-hidden">running for </span>
      {runClock(startedAt, now)}
    </span>
  );
}
