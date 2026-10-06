import { describe, expect, it } from "vitest";

import type { AuditRecordView } from "../bindings/AuditRecordView";
import type { EventView } from "../bindings/EventView";
import type { StartedTask } from "../bindings/StartedTask";
import { fixtureInvoke } from "./fixtures";

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
});
