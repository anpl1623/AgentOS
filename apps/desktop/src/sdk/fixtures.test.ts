import { describe, expect, it, vi } from "vitest";

import type { AuditRecordView } from "../bindings/AuditRecordView";
import type { CadenceInput } from "../bindings/CadenceInput";
import type { EventView } from "../bindings/EventView";
import type { MemoryView } from "../bindings/MemoryView";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { StartedTask } from "../bindings/StartedTask";
import { fixtureInvoke, fixtureSubscribe } from "./fixtures";

/**
 * Fixtures stand in for the runtime while screens are built, so they refuse
 * what the runtime refuses. A screen that has only ever met a fixture that
 * says yes has never shown its refusal.
 */
describe("fixtures", () => {
  it("retry only a task whose latest attempt failed or was cancelled", async () => {
    // task-0003's only attempt was abandoned, which is a failure.
    const started = await fixtureInvoke<StartedTask>("retry_task", { taskId: "task-0003" });
    expect(started.task_id).toBe("task-0003");

    // task-0002 failed once and then succeeded: retrying repeats its effects.
    await expect(fixtureInvoke("retry_task", { taskId: "task-0002" })).rejects.toMatch(/succeeded/);
    // task-0001 is still going.
    await expect(fixtureInvoke("retry_task", { taskId: "task-0001" })).rejects.toMatch(/running/);
    await expect(fixtureInvoke("retry_task", { taskId: "task-none" })).rejects.toMatch(/never been run/);
  });

  it("open an audit record only by an identity it has", async () => {
    const events = await fixtureInvoke<EventView[]>("activity", {});
    const first = events[0]!;
    const record = await fixtureInvoke<AuditRecordView>("audit_record", { eventId: first.id });
    expect(record.id).toBe(first.id);

    await expect(fixtureInvoke("audit_record", { eventId: "no-such-record" })).rejects.toMatch(
      /no audit record/,
    );
  });
  it("refuse a scheduler pace the runtime refuses, and accept one it allows", async () => {
    await expect(
      fixtureInvoke("set_scheduler_running", {
        enabled: true,
        tickSeconds: 4,
        maxConcurrentRuns: 1,
      }),
    ).rejects.toMatch(/5 seconds/);
    await expect(
      fixtureInvoke("set_scheduler_running", {
        enabled: true,
        tickSeconds: 30,
        maxConcurrentRuns: 0,
      }),
    ).rejects.toMatch(/at least one run/);
    const started = await fixtureInvoke<SchedulerView>("set_scheduler_running", {
      enabled: true,
      tickSeconds: 5,
      maxConcurrentRuns: 1,
    });
    expect(started.running).toBe(true);
  });

  it("refuse a memory kind the runtime does not have, and record a note as the operator's", async () => {
    await expect(
      fixtureInvoke("list_memories", { agentId: "agent-sales", kind: "rumour" }),
    ).rejects.toMatch(/not a kind of memory/);
    const remembered = await fixtureInvoke<MemoryView>("remember", {
      input: {
        agent_id: "agent-sales",
        kind: "fact",
        content: "x",
        confidence: null,
        source: "web",
      },
    });
    expect(remembered.source).toBe("user");
    expect(remembered.source_untrusted).toBe(false);
  });

  it("refuse a cadence that names two kinds, and a one-off with no time", async () => {
    const twoKinds: CadenceInput = {
      kind: "every",
      seconds: 3600,
      expression: "0 9 * * *",
      clock: null,
    };
    await expect(fixtureInvoke("check_cadence", { cadence: twoKinds })).rejects.toMatch(
      /exactly one/,
    );

    const once: CadenceInput = { kind: "once", seconds: null, expression: null, clock: null };
    await expect(
      fixtureInvoke("create_schedule", {
        input: {
          agent_id: "agent-sales",
          name: "n",
          objective: "o",
          cadence: once,
          first_run_at: null,
        },
      }),
    ).rejects.toMatch(/needs a time/);
  });

  it("replay live events without a place in the chain, as the real stream sends them", () => {
    vi.useFakeTimers();
    try {
      const received: EventView[] = [];
      const stop = fixtureSubscribe<EventView>("agentos://activity", (event) =>
        received.push(event),
      );
      vi.advanceTimersByTime(6_000);
      stop();
      expect(received.length).toBeGreaterThan(0);
      expect(received.every((event) => event.sequence === null)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
