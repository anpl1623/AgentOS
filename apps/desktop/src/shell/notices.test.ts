import { describe, expect, it } from "vitest";

import type { EventView } from "../bindings/EventView";
import {
  REASON_LIMIT,
  RUN_FAILED_KIND,
  alertForEvent,
  alertForStoreError,
  storeAlertId,
} from "./notices";

function event(kind: string, extra: Partial<EventView> = {}): EventView {
  return {
    id: "event-1",
    sequence: null,
    at: "2026-10-06T00:00:00Z",
    kind,
    run_id: "run-0001",
    task_id: "task-0001",
    summary: "model provider returned 529",
    security_relevant: false,
    ...extra,
  };
}

describe("alertForEvent", () => {
  it("raises a failed run as a warning that leaves by itself and links to the run", () => {
    const alert = alertForEvent(event(RUN_FAILED_KIND), { name: "dashboard" });
    expect(alert).toMatchObject({
      level: "warn",
      sticky: false,
      id: "run-failed:run-0001",
      message: "A run failed",
      detail: "model provider returned 529",
    });
    expect(alert?.link).toEqual({
      label: "Open run",
      route: { name: "tasks", runId: "run-0001" },
    });
  });

  it("raises nothing for success, approvals or refusals", () => {
    for (const kind of [
      "agent.task.completed",
      "agent.task.started",
      "approval.requested",
      "approval.granted",
      "permission.denied",
      "agent.task.cancelled",
    ]) {
      expect(alertForEvent(event(kind), { name: "dashboard" })).toBeNull();
    }
  });

  it("stays quiet when the operator is already looking at that run", () => {
    expect(alertForEvent(event(RUN_FAILED_KIND), { name: "tasks", runId: "run-0001" })).toBeNull();
    expect(
      alertForEvent(event(RUN_FAILED_KIND), { name: "tasks", runId: "run-0002" }),
    ).not.toBeNull();
    expect(alertForEvent(event(RUN_FAILED_KIND), { name: "tasks" })).not.toBeNull();
  });

  it("keys the card by run, so a re-delivered event replaces it", () => {
    const first = alertForEvent(event(RUN_FAILED_KIND, { id: "a" }), { name: "dashboard" });
    const again = alertForEvent(event(RUN_FAILED_KIND, { id: "b" }), { name: "dashboard" });
    expect(first?.id).toBe(again?.id);
  });

  it("shortens a long reason, which the trace has in full", () => {
    const alert = alertForEvent(event(RUN_FAILED_KIND, { summary: "x".repeat(1000) }), {
      name: "dashboard",
    });
    expect(alert?.detail?.length).toBe(REASON_LIMIT);
  });

  it("still raises, without a link, for a failure that names no run", () => {
    const alert = alertForEvent(event(RUN_FAILED_KIND, { run_id: null, summary: "" }), {
      name: "dashboard",
    });
    expect(alert?.message).toBe("A run failed");
    expect(alert?.detail ?? null).toBeNull();
    expect(alert?.link ?? null).toBeNull();
  });
});

describe("alertForStoreError", () => {
  it("is an error under the store's id, so recovery can dismiss it", () => {
    const alert = alertForStoreError("approvals", "connection refused");
    expect(alert.level).toBe("error");
    expect(alert.id).toBe(storeAlertId("approvals"));
    expect(alert.detail).toBe("connection refused");
    expect(alert.link ?? null).toBeNull();
    expect(storeAlertId("approvals")).not.toBe(storeAlertId("events"));
  });
});

describe("an alert's link", () => {
  it("is data naming a route, never a function, so an alert can only navigate", () => {
    const alert = alertForEvent(event(RUN_FAILED_KIND), { name: "dashboard" });
    const values = Object.values(alert ?? {}).concat(Object.values(alert?.link ?? {}));
    expect(values.some((value) => typeof value === "function")).toBe(false);
  });
});
