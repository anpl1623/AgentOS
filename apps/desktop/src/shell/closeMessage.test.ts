import { describe, expect, it } from "vitest";

import { closeMessage } from "./closeMessage";

const quiet = { live_runs: 0, pending_approvals: 0, scheduler_running: false };

describe("closeMessage", () => {
  it("names each live thing in its own paragraph, in a fixed order", () => {
    const message = closeMessage(
      { live_runs: 2, pending_approvals: 1, scheduler_running: true },
      null,
    );
    const paragraphs = message.split("\n\n");
    expect(paragraphs).toHaveLength(3);
    expect(paragraphs[0]).toMatch(/^2 runs will be stopped/);
    expect(paragraphs[1]).toMatch(/^1 approval is waiting on you\./);
    expect(paragraphs[2]).toMatch(/^The scheduler stops\./);
  });

  it("counts in the singular and the plural", () => {
    expect(closeMessage({ ...quiet, live_runs: 1 }, null)).toMatch(/^1 run will be stopped/);
    expect(closeMessage({ ...quiet, pending_approvals: 3 }, null)).toMatch(
      /^3 approvals are waiting on you\. They can no longer/,
    );
  });

  it("adds why the window did not close, last", () => {
    const message = closeMessage({ ...quiet, live_runs: 1 }, "the runtime is busy");
    expect(message.split("\n\n").at(-1)).toBe(
      "The window could not be closed: the runtime is busy",
    );
  });

  it("says nothing when nothing is live", () => {
    expect(closeMessage(quiet, null)).toBe("");
  });
});
