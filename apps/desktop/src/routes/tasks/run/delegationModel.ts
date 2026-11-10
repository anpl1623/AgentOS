/**
 * Delegation on the run screen: which call asked for which runs, and the rows
 * drawn beneath it.
 *
 * A run that delegated is answerable for what it asked for, so every run
 * beneath it is drawn under the call that asked, however far down, in its own
 * state and with its own taint.
 */

import type { DelegationTreeView } from "../../../bindings/DelegationTreeView";
import type { DelegationView } from "../../../bindings/DelegationView";
import type { ExecutionView } from "../../../bindings/ExecutionView";
import { type VerdictTone, runTone } from "../../../components/status";
import { instant } from "../../../sdk/eventStream";
import { type Entry, compareInstants, mergeEntries } from "./runModel";

/** The tool through which one agent asks another to do part of its work. */
export const DELEGATE_TOOL = "agent.delegate";

/**
 * How far below a run its delegation tree is walked.
 *
 * The runtime bounds the tree it returns by the configured depth, which is
 * two; this is the window's own bound, matching the sixteen hops the store
 * walks an ancestry, so a malformed answer cannot recurse without end.
 */
export const MAX_DELEGATION_DEPTH = 16;

/** The agents and objectives one `agent.delegate` call asked for, as its arguments say. */
export function delegateRequests(json: string): { agent: string; objective: string }[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return [];
  }
  const requests = (parsed as { requests?: unknown } | null)?.requests;
  if (!Array.isArray(requests)) return [];
  return requests.flatMap((request: unknown) => {
    const { agent, objective } = (request ?? {}) as { agent?: unknown; objective?: unknown };
    return typeof agent === "string" && typeof objective === "string" ? [{ agent, objective }] : [];
  });
}

/** Which call each delegation belongs to, and those no recorded call accounts for. */
export interface DelegationPlacement {
  /** By execution id, in the order the call asked. */
  byExecution: Map<string, DelegationView[]>;
  /**
   * Delegations no recorded call asked for. A call is recorded when it ends,
   * so on a live run these are the work of the call still in progress.
   */
  unplaced: DelegationView[];
}

/**
 * Each delegation under the `agent.delegate` call that made it.
 *
 * Matched by what the call asked for, not by time. A delegation names neither
 * its call nor the moment the call ended, and a call still in progress has no
 * record yet; a window of time would hand that call's delegations to the
 * finished call before it. Its arguments name each agent and objective it
 * asked for, and the runtime writes the delegations in that order, so the
 * calls are walked in the order they started and each request takes the
 * earliest unclaimed delegation it describes that was not recorded before
 * the call began. A time that cannot be read rules nothing out.
 */
export function placeDelegations(
  executions: readonly ExecutionView[],
  delegations: readonly DelegationView[],
): DelegationPlacement {
  const pool = [...delegations]
    .map((delegation, order) => ({ delegation, order, ms: instant(delegation.requested_at) }))
    .sort((a, b) => compareInstants(a.ms, b.ms) || a.order - b.order);
  const claimed = new Set<number>();
  const byExecution = new Map<string, DelegationView[]>();
  const calls = mergeEntries([], executions).filter(
    (entry): entry is Extract<Entry, { kind: "execution" }> =>
      entry.kind === "execution" && entry.execution.tool === DELEGATE_TOOL,
  );
  for (const { execution, ms: started } of calls) {
    const placed: DelegationView[] = [];
    for (const request of delegateRequests(execution.arguments)) {
      const match = pool.find(
        (candidate) =>
          !claimed.has(candidate.order) &&
          candidate.delegation.child_agent === request.agent &&
          candidate.delegation.objective === request.objective &&
          !(Number.isFinite(started) && candidate.ms < started),
      );
      if (match === undefined) continue;
      claimed.add(match.order);
      placed.push(match.delegation);
    }
    if (placed.length > 0) byExecution.set(execution.id, placed);
  }
  return {
    byExecution,
    unplaced: pool.filter((each) => !claimed.has(each.order)).map((each) => each.delegation),
  };
}

/** Delegation states that are not yet an outcome. */
const OPEN_DELEGATION = new Set(["requested", "running"]);

const DELEGATION_TONES = {
  requested: "live",
  running: "live",
  completed: "ok",
  failed: "danger",
  refused: "blocked",
  cancelled: "neutral",
} as const satisfies Record<string, VerdictTone>;

/**
 * A delegation's state as a verdict tone.
 *
 * A refusal is the accent of a policy or a limit working, as a refused call
 * is; a child that failed is a failure, though the parent goes on. A state
 * this build does not know is neutral, never a guess at a verdict.
 */
export function delegationTone(state: string): VerdictTone {
  return Object.hasOwn(DELEGATION_TONES, state)
    ? DELEGATION_TONES[state as keyof typeof DELEGATION_TONES]
    : "neutral";
}

/** One line beneath a call: a run another run asked for. */
export interface DelegationRow {
  key: string;
  /** 1 for a run this run asked for, 2 for one that run asked for, and so on. */
  depth: number;
  /** The agent that asked, so depth is said in words as well as drawn. */
  askedBy: string;
  agent: string;
  objective: string;
  state: string;
  tone: VerdictTone;
  steps: number;
  tainted: boolean;
  /** The child's run, when one was prepared; its trace is a link. */
  runId: string | null;
}

/**
 * The rows beneath a call: each delegation it made, followed by every run
 * delegated beneath that one, depth first.
 *
 * A direct delegation that has ended is shown in its own state, which is the
 * outcome the asking run was given, and says whether it was refused before a
 * run existed. One still under way is shown in its run's state, as the
 * command line shows it; and a run further down in its run state, since the
 * tree carries runs. `tree` is this run's delegation tree,
 * or `null` before it has loaded, when only the direct rows are known.
 */
export function delegationRows(
  asker: string,
  delegations: readonly DelegationView[],
  tree: DelegationTreeView | null,
): DelegationRow[] {
  const rows: DelegationRow[] = [];
  const seen = new Set<string>(tree === null ? [] : [tree.run_id]);
  const below = (node: DelegationTreeView, depth: number) => {
    if (depth > MAX_DELEGATION_DEPTH) return;
    for (const child of node.children) {
      if (seen.has(child.run_id)) continue;
      seen.add(child.run_id);
      rows.push({
        key: `run-${child.run_id}`,
        depth,
        askedBy: node.agent,
        agent: child.agent,
        objective: child.objective,
        state: child.state,
        tone: runTone(child.state),
        steps: child.steps,
        tainted: child.tainted,
        runId: child.run_id,
      });
      below(child, depth + 1);
    }
  };
  for (const delegation of delegations) {
    const node =
      delegation.child_run_id === null
        ? undefined
        : tree?.children.find((child) => child.run_id === delegation.child_run_id);
    // Under way, the child's run says how far it has got, which "running"
    // does not: waiting on a person is the state an operator is looking for.
    const underWay = node !== undefined && OPEN_DELEGATION.has(delegation.state);
    rows.push({
      key: `delegation-${delegation.id}`,
      depth: 1,
      askedBy: asker,
      agent: delegation.child_agent,
      objective: delegation.objective,
      state: underWay ? node.state : delegation.state,
      tone: underWay ? runTone(node.state) : delegationTone(delegation.state),
      steps: delegation.steps,
      tainted: delegation.tainted,
      runId: delegation.child_run_id,
    });
    if (node !== undefined && !seen.has(node.run_id)) {
      seen.add(node.run_id);
      below(node, 2);
    }
  }
  return rows;
}

/** Every run in a delegation tree, the root included. */
export function treeRunIds(tree: DelegationTreeView | null): Set<string> {
  const ids = new Set<string>();
  const walk = (node: DelegationTreeView, depth: number) => {
    if (depth > MAX_DELEGATION_DEPTH || ids.has(node.run_id)) return;
    ids.add(node.run_id);
    for (const child of node.children) walk(child, depth + 1);
  };
  if (tree !== null) walk(tree, 0);
  return ids;
}
