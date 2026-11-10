/**
 * Which family an event belongs to, how loudly it is drawn, and which icon
 * marks it, from its dotted kind alone.
 *
 * The runtime names its events `area.subject.verb`, and the first segments say
 * what part of the system spoke: a task, the model, the permission engine, a
 * tool. That is what an eye scanning a long feed wants first, so each family
 * gets one icon and the filter chips are families rather than the forty-odd
 * kinds. The kinds themselves are `AgentEvent::kind` in agentos-core; a kind
 * this file has never heard of still lands somewhere, under Other.
 */

import type { EventView } from "../../bindings/EventView";
import type { IconName } from "../../components/icons";

/** The parts of the system an event can come from. */
export type EventFamily =
  | "task"
  | "state"
  | "model"
  | "permission"
  | "approval"
  | "tool"
  | "taint"
  | "delegation"
  | "memory"
  | "schedule"
  | "operator"
  | "network"
  | "other";

/** How a family is shown: its chip's words and the icon on each of its rows. */
export interface FamilyInfo {
  id: EventFamily;
  label: string;
  icon: IconName;
}

/**
 * Every family, in the order the chips offer them: the run's own life first,
 * then what was asked and allowed, then what the operator and the outside
 * world did.
 */
export const FAMILIES: readonly FamilyInfo[] = [
  { id: "task", label: "Tasks", icon: "tasks" },
  { id: "state", label: "State", icon: "zap" },
  { id: "model", label: "Model", icon: "cpu" },
  { id: "permission", label: "Permissions", icon: "lock" },
  { id: "tool", label: "Tools", icon: "terminal" },
  { id: "taint", label: "Taint", icon: "alert" },
  { id: "approval", label: "Approvals", icon: "approvals" },
  { id: "delegation", label: "Delegation", icon: "link" },
  { id: "memory", label: "Memory", icon: "edit" },
  { id: "schedule", label: "Schedules", icon: "schedules" },
  { id: "operator", label: "Operator", icon: "user" },
  { id: "network", label: "Network", icon: "globe" },
  { id: "other", label: "Other", icon: "activity" },
];

const BY_ID = new Map(FAMILIES.map((family) => [family.id, family]));

/**
 * Kind prefixes, most specific first, so `operator.task.` is a task before it
 * is the operator's and `operator.memory.` is memory before either.
 */
const PREFIXES: readonly (readonly [string, EventFamily])[] = [
  ["agent.task.", "task"],
  ["operator.task.", "task"],
  ["agent.state.", "state"],
  ["agent.model.", "model"],
  ["agent.reasoning.", "model"],
  ["permission.", "permission"],
  ["approval.", "approval"],
  ["tool.", "tool"],
  ["agent.taint.", "taint"],
  ["agent.delegation.", "delegation"],
  ["agent.memory.", "memory"],
  ["operator.memory.", "memory"],
  ["schedule.", "schedule"],
  ["operator.schedule.", "schedule"],
  ["operator.scheduler.", "schedule"],
  ["operator.", "operator"],
  ["network.", "network"],
];

/** Whether a string names a family; for a choice that arrives as data. */
export function isEventFamily(value: string): value is EventFamily {
  return BY_ID.has(value as EventFamily);
}

/** The family a kind belongs to. */
export function familyOf(kind: string): EventFamily {
  for (const [prefix, family] of PREFIXES) {
    if (kind.startsWith(prefix)) return family;
  }
  return "other";
}

/** A family's chip words and icon. */
export function familyInfo(family: EventFamily): FamilyInfo {
  // Every member of the union is in the table; the fallback is for the type.
  return BY_ID.get(family) ?? { id: "other", label: "Other", icon: "activity" };
}

/**
 * Kinds whose own icon says more than their family's: a policy change is a
 * shield, a key or credential a key, an integration a link.
 */
const KIND_ICONS: readonly (readonly [string, IconName])[] = [
  ["operator.policy.", "shield"],
  ["operator.provider_key.", "key"],
  ["operator.credential.", "key"],
  ["network.credential.", "key"],
  ["operator.integration.", "link"],
  ["operator.agent.", "agents"],
];

/** The icon on an event's row. */
export function eventIcon(kind: string): IconName {
  for (const [prefix, icon] of KIND_ICONS) {
    if (kind.startsWith(prefix)) return icon;
  }
  return familyInfo(familyOf(kind)).icon;
}

/**
 * How an event's mark is coloured. Each is a status hue from the tokens:
 * `blocked` is a refusal by policy (a policy that refuses has done its job, so
 * it is not danger), `warn` is attention (untrusted data, a decision waiting,
 * any other security-relevant change), `danger` a failure, `ok` work finished
 * or allowed by a person, `info` something starting, `neutral` the rest.
 */
export type EventTone = "neutral" | "ok" | "info" | "warn" | "danger" | "blocked";

/** Kinds that record the runtime refusing something. */
const REFUSALS = new Set([
  "permission.denied",
  "approval.denied",
  "tool.unknown",
  "tool.arguments.rejected",
  "tool.manifest_exceeded",
  "agent.delegation.refused",
]);

/** Kinds that record a run reading, or acting on, untrusted data. */
const TAINT = new Set(["agent.taint.raised", "permission.escalated_by_taint"]);

/** Kinds that record work finished, or allowed by a person. */
const SETTLED = new Set(["agent.task.completed", "agent.delegation.completed", "approval.granted"]);

/**
 * The tone of an event's mark.
 *
 * In order of precedence: a refusal, a failure, taint, anything else the
 * runtime flags as security-relevant, a decision being asked for, work
 * settled, work starting. Routine events stay neutral, so that colour in a
 * long feed always means something.
 */
export function eventTone(event: Pick<EventView, "kind" | "security_relevant">): EventTone {
  const { kind } = event;
  if (REFUSALS.has(kind)) return "blocked";
  if (kind.endsWith(".failed")) return "danger";
  if (TAINT.has(kind)) return "warn";
  if (event.security_relevant) return "warn";
  if (kind === "approval.requested") return "warn";
  if (SETTLED.has(kind)) return "ok";
  if (kind.endsWith(".started")) return "info";
  return "neutral";
}

/** One chip: a family and how many events in view belong to it. */
export interface FamilyCount {
  family: EventFamily;
  label: string;
  icon: IconName;
  count: number;
}

/**
 * The family chips for a list of events, in {@link FAMILIES} order: every
 * family that has an event, with its count.
 *
 * A chosen family with nothing left in the list stays offered, at zero, so
 * the chip that is filtering the feed never disappears from under the
 * operator; the same rule `kindOptions` keeps for the kind select.
 */
export function familyCounts(
  events: readonly Pick<EventView, "kind">[],
  selected: EventFamily | "" = "",
): FamilyCount[] {
  const counts = new Map<EventFamily, number>();
  for (const event of events) {
    const family = familyOf(event.kind);
    counts.set(family, (counts.get(family) ?? 0) + 1);
  }
  return FAMILIES.filter(
    (family) => (counts.get(family.id) ?? 0) > 0 || family.id === selected,
  ).map((family) => ({
    family: family.id,
    label: family.label,
    icon: family.icon,
    count: counts.get(family.id) ?? 0,
  }));
}
