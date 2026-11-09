import { describe, expect, it } from "vitest";

import type { TaskSummary } from "../../bindings/TaskSummary";
import { DETAIL_SECTIONS, activeSection, scrolledToEnd, taskTally } from "./detailModel";

const positions = [
  { id: "a", top: 100 },
  { id: "b", top: 600 },
  { id: "c", top: 1200 },
];

describe("activeSection", () => {
  it("is the last heading to have risen to the reading line", () => {
    expect(activeSection(positions, 140, false)).toBe("a");
    expect(activeSection(positions.map((p) => ({ ...p, top: p.top - 500 })), 140, false)).toBe(
      "b",
    );
  });

  it("is the first section before any heading reaches the line", () => {
    expect(activeSection(positions.map((p) => ({ ...p, top: p.top + 400 })), 140, false)).toBe(
      "a",
    );
  });

  it("is the last section at the end of the page, even if its heading never rose", () => {
    expect(activeSection(positions, 140, true)).toBe("c");
  });

  it("is nothing when there are no sections", () => {
    expect(activeSection([], 140, false)).toBeNull();
  });
});

describe("scrolledToEnd", () => {
  it("allows a pixel or two of rounding", () => {
    expect(scrolledToEnd(500, 400, 900)).toBe(true);
    expect(scrolledToEnd(498.5, 400, 900)).toBe(true);
    expect(scrolledToEnd(400, 400, 900)).toBe(false);
  });
});

describe("DETAIL_SECTIONS", () => {
  it("names each heading once", () => {
    const ids = DETAIL_SECTIONS.map((section) => section.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("taskTally", () => {
  const task = (status: string): TaskSummary => ({
    id: `task-${status}`,
    objective: "Do it.",
    status,
    agent_name: "sales",
    agent_id: "id-sales",
    created_at: "2026-10-01T00:00:00Z",
    completed_at: null,
    latest_run: null,
  });

  it("counts the tasks and those running now", () => {
    expect(taskTally([task("running"), task("succeeded"), task("running")])).toEqual({
      total: 3,
      running: 2,
    });
    expect(taskTally([])).toEqual({ total: 0, running: 0 });
  });
});
