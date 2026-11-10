/**
 * The run as plain text, for pasting into a ticket or a message.
 *
 * A transcript is evidence that leaves the window, so a hostile string in it
 * must not be able to rearrange itself on paste, and every time in it reads
 * the same on every machine.
 */

import type { DelegationView } from "../../../bindings/DelegationView";
import type { TraceView } from "../../../bindings/TraceView";
import { visibleText } from "../../../components/argumentText";
import { instant } from "../../../sdk/eventStream";
import { humanise, isLive } from "../../../sdk/format";
import { DELEGATE_TOOL, type DelegationPlacement, placeDelegations } from "./delegationModel";
import { type Entry, mergeEntries } from "./runModel";
import { TAINT_LABEL } from "./timelineModel";

/**
 * A time as `HH:MM:SS` on the local 24-hour clock, fixed rather than in the
 * locale's format, so a pasted transcript reads the same on every machine. A
 * time that cannot be read is given as it was written.
 */
export function hhmmss(iso: string): string {
  const ms = instant(iso);
  if (!Number.isFinite(ms)) return iso;
  const at = new Date(ms);
  return [at.getHours(), at.getMinutes(), at.getSeconds()]
    .map((part) => String(part).padStart(2, "0"))
    .join(":");
}

/**
 * Text from the run, safe to put in a transcript.
 *
 * The summaries and errors are the model's and the tools' words, and a tool
 * may have read a hostile page. A direction override pasted into a ticket
 * reorders what the reader sees, so every invisible or direction-changing
 * code point is written as its visible escape, the same one the argument
 * block draws. Control characters are removed later, by the copy itself.
 */
function plain(text: string): string {
  return visibleText(text);
}

/** Lines after the first, indented under the line they belong to. */
function continued(text: string, indent: string): string {
  return text
    .split("\n")
    .map((line, index) => (index === 0 ? line : `${indent}${line}`))
    .join("\n");
}

const INDENT = "    ";

/**
 * One delegation as a transcript line, in the shape the command line prints:
 * agent, state, steps, and taint when there is any.
 */
function delegationLine(delegation: DelegationView): string {
  const parts = [
    plain(delegation.child_agent),
    humanise(delegation.state),
    `${delegation.steps} steps`,
    ...(delegation.tainted ? [TAINT_LABEL] : []),
  ];
  return `${INDENT}└─ ${parts.join(" · ")}`;
}

/**
 * One entry as one transcript line, its error indented beneath, and beneath
 * a delegating call the delegations it made.
 */
function entryLine(entry: Entry, placement: DelegationPlacement): string {
  if (entry.kind === "step") {
    const { step } = entry;
    return `${hhmmss(step.at)} ${step.kind} ${continued(plain(step.summary), INDENT)}`;
  }
  const { execution } = entry;
  const lines = [
    `${hhmmss(execution.started_at)} tool ${plain(execution.tool)} ` +
      `decision=${execution.effect} outcome=${execution.outcome}`,
  ];
  if (execution.error) lines.push(`${INDENT}${continued(plain(execution.error), INDENT)}`);
  for (const delegation of placement.byExecution.get(execution.id) ?? []) {
    lines.push(delegationLine(delegation));
  }
  return lines.join("\n");
}

/**
 * The run as plain text, for pasting into a ticket or a message.
 *
 * Objective, agent, the run that started this one if another did, run,
 * attempt, state, taint and tokens, then the failure or result when there is
 * one, then one line per entry of the merged timeline: `HH:MM:SS kind
 * summary`, a tool call annotated with its decision and outcome, an error
 * indented beneath the call that raised it, and the delegations a call made
 * beneath it.
 */
export function transcript(trace: TraceView, shown: string = trace.run.state): string {
  const { run } = trace;
  const taint = run.tainted
    ? run.taint_sources.length > 0
      ? `${TAINT_LABEL} from ${run.taint_sources.map(plain).join(", ")}`
      : TAINT_LABEL
    : "none";
  const head = [
    `Objective: ${continued(plain(trace.objective), INDENT)}`,
    `Agent: ${plain(trace.agent_name)}`,
    `Run: ${run.id}`,
    `Attempt: ${run.attempt}`,
    `State: ${humanise(shown)}`,
    `Taint: ${taint}`,
    `Tokens: ${run.input_tokens} in, ${run.output_tokens} out`,
  ];
  if (trace.delegated_by) {
    const by = trace.delegated_by;
    head.splice(2, 0, `Started by: ${plain(by.parent_agent)}, run ${by.parent_run_id}`);
  }
  if (run.failure) head.push(`Failure: ${continued(plain(run.failure), INDENT)}`);
  if (run.result) head.push(`Result: ${continued(plain(run.result), INDENT)}`);
  const placement = placeDelegations(trace.executions, trace.delegations);
  const entries = mergeEntries(trace.steps, trace.executions).map((entry) =>
    entryLine(entry, placement),
  );
  if (placement.unplaced.length > 0) {
    entries.push(
      isLive(run.state)
        ? `${DELEGATE_TOOL} in progress`
        : `${DELEGATE_TOOL} with no recorded call`,
      ...placement.unplaced.map(delegationLine),
    );
  }
  return `${[...head, "", ...entries].join("\n")}\n`;
}
