import { describe, expect, it } from "vitest";

import { ICON_NAMES } from "../../components/icons";
import {
  FAMILIES,
  eventIcon,
  eventTone,
  familyCounts,
  familyInfo,
  familyOf,
  isEventFamily,
} from "./eventFamilies";

/**
 * Every kind `AgentEvent::kind` writes today (crates/agentos-core/src/event/
 * kind.rs), with the family the feed should file it under. A kind added there
 * and not here still lands in a family; this list is what keeps the families
 * honest about the ones that exist.
 */
const KINDS: readonly (readonly [string, string])[] = [
  ["agent.task.started", "task"],
  ["agent.task.completed", "task"],
  ["agent.task.failed", "task"],
  ["agent.task.cancelled", "task"],
  ["agent.task.abandoned", "task"],
  ["agent.state.transitioned", "state"],
  ["agent.model.request.started", "model"],
  ["agent.model.request.completed", "model"],
  ["agent.model.request.failed", "model"],
  ["agent.reasoning.completed", "model"],
  ["permission.requested", "permission"],
  ["permission.granted", "permission"],
  ["permission.denied", "permission"],
  ["permission.escalated_by_taint", "permission"],
  ["agent.taint.raised", "taint"],
  ["approval.requested", "approval"],
  ["approval.granted", "approval"],
  ["approval.denied", "approval"],
  ["tool.execution.started", "tool"],
  ["tool.execution.completed", "tool"],
  ["tool.execution.failed", "tool"],
  ["tool.arguments.rejected", "tool"],
  ["tool.unknown", "tool"],
  ["tool.manifest_exceeded", "tool"],
  ["agent.memory.recorded", "memory"],
  ["schedule.fired", "schedule"],
  ["agent.delegation.started", "delegation"],
  ["agent.delegation.completed", "delegation"],
  ["agent.delegation.refused", "delegation"],
  ["operator.policy.changed", "operator"],
  ["operator.agent.created", "operator"],
  ["operator.agent.enabled_changed", "operator"],
  ["operator.provider_key.set", "operator"],
  ["operator.provider_key.removed", "operator"],
  ["operator.credential.set", "operator"],
  ["operator.credential.removed", "operator"],
  ["operator.integration.bound", "operator"],
  ["operator.integration.unbound", "operator"],
  ["operator.memory.recorded", "memory"],
  ["operator.memory.revised", "memory"],
  ["operator.memory.forgotten", "memory"],
  ["operator.schedule.created", "schedule"],
  ["operator.schedule.paused", "schedule"],
  ["operator.schedule.resumed", "schedule"],
  ["operator.schedule.deleted", "schedule"],
  ["network.credential.used", "network"],
  ["operator.task.created", "task"],
  ["operator.task.dependency_added", "task"],
  ["operator.scheduler.started", "schedule"],
  ["operator.scheduler.stopped", "schedule"],
];

describe("familyOf", () => {
  it("files every kind the runtime writes under the family it belongs to", () => {
    for (const [kind, family] of KINDS) {
      expect([kind, familyOf(kind)]).toEqual([kind, family]);
    }
  });

  it("files the operator's changes to tasks, memory and schedules with those, not with the operator", () => {
    expect(familyOf("operator.task.created")).toBe("task");
    expect(familyOf("operator.memory.revised")).toBe("memory");
    expect(familyOf("operator.scheduler.stopped")).toBe("schedule");
    expect(familyOf("operator.policy.changed")).toBe("operator");
  });

  it("files a kind it has never heard of under Other rather than dropping it", () => {
    expect(familyOf("runtime.something.new")).toBe("other");
    expect(familyOf("")).toBe("other");
  });

  it("does not take a family from a prefix that only looks alike", () => {
    // "tooling." is not "tool.", and "permissions." is not "permission.".
    expect(familyOf("tooling.loaded")).toBe("other");
    expect(familyOf("permissions.reset")).toBe("other");
  });
});

describe("families", () => {
  it("each have a distinct label and an icon that exists", () => {
    const labels = FAMILIES.map((family) => family.label);
    expect(new Set(labels).size).toBe(labels.length);
    for (const family of FAMILIES) {
      expect(ICON_NAMES).toContain(family.icon);
      expect(familyInfo(family.id)).toBe(family);
    }
  });

  it("recognises its own names and nothing else", () => {
    expect(isEventFamily("tool")).toBe(true);
    expect(isEventFamily("tools")).toBe(false);
    expect(isEventFamily("")).toBe(false);
  });
});

describe("eventIcon", () => {
  it("uses the family's icon unless the kind has a better one", () => {
    expect(eventIcon("tool.execution.completed")).toBe("terminal");
    expect(eventIcon("operator.policy.changed")).toBe("shield");
    expect(eventIcon("operator.credential.set")).toBe("key");
    expect(eventIcon("network.credential.used")).toBe("key");
    expect(eventIcon("operator.agent.created")).toBe("agents");
  });

  it("gives every kind the runtime writes an icon that exists", () => {
    for (const [kind] of KINDS) expect(ICON_NAMES).toContain(eventIcon(kind));
  });
});

describe("eventTone", () => {
  const tone = (kind: string, security_relevant = false) => eventTone({ kind, security_relevant });

  it("draws a refusal as blocked, not as danger", () => {
    for (const kind of [
      "permission.denied",
      "approval.denied",
      "tool.unknown",
      "tool.arguments.rejected",
      "tool.manifest_exceeded",
      "agent.delegation.refused",
    ]) {
      expect([kind, tone(kind, true)]).toEqual([kind, "blocked"]);
    }
  });

  it("draws a failure as danger", () => {
    expect(tone("agent.task.failed")).toBe("danger");
    expect(tone("tool.execution.failed")).toBe("danger");
    expect(tone("agent.model.request.failed")).toBe("danger");
  });

  it("asks for attention for taint, other security-relevant changes and a decision waiting", () => {
    expect(tone("agent.taint.raised", true)).toBe("warn");
    expect(tone("permission.escalated_by_taint", true)).toBe("warn");
    expect(tone("operator.policy.changed", true)).toBe("warn");
    expect(tone("approval.requested")).toBe("warn");
  });

  it("marks work settled and work starting, and leaves routine events neutral", () => {
    expect(tone("agent.task.completed")).toBe("ok");
    expect(tone("approval.granted")).toBe("ok");
    expect(tone("agent.task.started")).toBe("info");
    expect(tone("tool.execution.completed")).toBe("neutral");
    expect(tone("permission.granted")).toBe("neutral");
    expect(tone("agent.state.transitioned")).toBe("neutral");
  });

  it("lets a refusal outrank the security flag, which every refusal also carries", () => {
    expect(tone("permission.denied", true)).toBe("blocked");
    expect(tone("permission.denied", false)).toBe("blocked");
  });
});

describe("familyCounts", () => {
  const events = [
    { kind: "tool.execution.completed" },
    { kind: "permission.denied" },
    { kind: "tool.unknown" },
    { kind: "agent.task.started" },
  ];

  it("counts each family present, in the chips' order", () => {
    expect(familyCounts(events).map(({ family, count }) => [family, count])).toEqual([
      ["task", 1],
      ["permission", 1],
      ["tool", 2],
    ]);
  });

  it("keeps the chosen family offered at zero after its events have gone", () => {
    expect(familyCounts(events, "approval").map(({ family, count }) => [family, count])).toEqual([
      ["task", 1],
      ["permission", 1],
      ["tool", 2],
      ["approval", 0],
    ]);
  });

  it("offers nothing for nothing", () => {
    expect(familyCounts([])).toEqual([]);
  });
});
