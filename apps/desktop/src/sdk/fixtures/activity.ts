/** The activity feed, the audit chain it is sealed in, and its live replay. */

import type { AuditHealth } from "../../bindings/AuditHealth";
import type { AuditRecordView } from "../../bindings/AuditRecordView";
import type { EventView } from "../../bindings/EventView";

import { agents } from "./agents";
import { minutesAgo } from "./clock";

export const activity: EventView[] = [
  ["agent.task.started", "Find overdue follow-ups in the CRM", false],
  ["agent.state.transitioned", "idle → planning", false],
  ["agent.model.request.completed", "anthropic/claude-opus-5", false],
  ["permission.granted", "browser.navigate", false],
  ["tool.execution.completed", "browser.navigate", false],
  ["agent.taint.raised", "browser.extract", true],
  ["permission.denied", "filesystem.read", true],
  ["tool.unknown", "terminal.exec", true],
  ["permission.escalated_by_taint", "browser.type", true],
  ["approval.requested", "browser.type", false],
].map(([kind, summary, security], index) => ({
  id: `event-${index}`,
  sequence: index + 41,
  at: minutesAgo(10 - index),
  kind: kind as string,
  run_id: "run-0001",
  task_id: "task-0001",
  summary: summary as string,
  security_relevant: security as boolean,
}));

export const auditHealth: AuditHealth = {
  events: 412,
  intact: true,
  unrecorded: 0,
  checked_at: minutesAgo(0),
};

/** A stand-in for a SHA-256 digest: 64 hex digits, distinct per record. */
const digest = (sequence: number) =>
  (sequence.toString(16).padStart(8, "0") + "9e3c1fa7b2d4").repeat(4).slice(0, 64);

/** The payload each activity fixture would have been sealed with. */
const payloads: Record<string, Record<string, unknown>> = {
  "permission.denied": {
    event: "permission.denied",
    tool: "filesystem.read",
    capability: {
      domain: "filesystem",
      action: "read",
      resource: { kind: "path", path: "/Users/me/.ssh/id_rsa" },
    },
    reason: "no rule matched `filesystem.read on path:/Users/me/.ssh/id_rsa`",
    matched_rule: null,
  },
};

/** Every activity fixture as the chain stores it, linked hash to hash. */
export const auditRecords: AuditRecordView[] = activity.map((event) => {
  const sequence = event.sequence ?? 0;
  return {
    id: event.id,
    sequence,
    at: event.at,
    kind: event.kind,
    agent_id: agents[0]!.id,
    task_id: event.task_id,
    run_id: event.run_id,
    payload: JSON.stringify(
      payloads[event.kind] ?? { event: event.kind, summary: event.summary },
      null,
      2,
    ),
    prev_hash: digest(sequence - 1),
    hash: digest(sequence),
    security_relevant: event.security_relevant,
  };
});

/**
 * Replay activity events on a timer, so the live feed can be seen working.
 *
 * Each replay arrives as the real stream delivers an event: with no sequence,
 * which the bridge in `state.rs` never sends. Replaying the stored sequence
 * would file live events among history and hide the feed's not-yet-written
 * state from anyone building against fixtures.
 */
export function fixtureSubscribe<T>(event: string, handler: (payload: T) => void): () => void {
  if (event !== "agentos://activity") return () => {};

  let index = 0;
  const timer = setInterval(() => {
    const next = activity[index % activity.length];
    if (next) {
      handler({
        ...next,
        id: `live-${index}`,
        sequence: null,
        at: new Date().toISOString(),
      } satisfies EventView as T);
    }
    index += 1;
  }, 3000);

  return () => clearInterval(timer);
}
