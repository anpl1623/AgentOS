import { describe, expect, it } from "vitest";

import {
  NAMED_CHARS,
  confidenceBand,
  confidenceLabel,
  confidencePercent,
  filterMemories,
  memoryRevision,
  shortName,
} from "./memoryModel";
import { memory } from "./testData";

describe("filterMemories", () => {
  const memories = [
    memory("1", { kind: "preference", content: "Never send without approval." }),
    memory("2", {
      kind: "observation",
      content: "Globex wants exports sent elsewhere.",
      source: "web:http://127.0.0.1:8420/customers/globex",
    }),
  ];

  it("filters by kind and by content or source", () => {
    expect(filterMemories(memories, "", null).map((m) => m.id)).toEqual(["1", "2"]);
    expect(filterMemories(memories, "", "observation").map((m) => m.id)).toEqual(["2"]);
    expect(filterMemories(memories, "APPROVAL", null).map((m) => m.id)).toEqual(["1"]);
    expect(filterMemories(memories, "globex", null).map((m) => m.id)).toEqual(["2"]);
    expect(filterMemories(memories, "8420", "preference")).toEqual([]);
  });
});

describe("confidenceLabel", () => {
  it("prints at most two places", () => {
    expect(confidenceLabel(1)).toBe("confidence 1");
    expect(confidenceLabel(0.4)).toBe("confidence 0.4");
    expect(confidenceLabel(0.123)).toBe("confidence 0.12");
  });

  it("clamps what the runtime could not have stored", () => {
    expect(confidenceLabel(1.5)).toBe("confidence 1");
    expect(confidenceLabel(-0.2)).toBe("confidence 0");
    expect(confidenceLabel(Number.NaN)).toBe("confidence unknown");
  });
});

describe("confidence meter", () => {
  it("fills to the confidence, clamped as the label is", () => {
    expect([1, 0.4, 0.123, 1.5, -0.2].map(confidencePercent)).toEqual([100, 40, 12, 100, 0]);
    expect(confidencePercent(Number.NaN)).toBe(0);
  });

  it("bands coarsely, with an unknown confidence counted as low", () => {
    expect([1, 0.8, 0.79, 0.5, 0.49, 0].map(confidenceBand)).toEqual([
      "high",
      "high",
      "medium",
      "medium",
      "low",
      "low",
    ]);
    expect(confidenceBand(Number.NaN)).toBe("low");
    expect(confidenceBand(7)).toBe("high");
  });
});

describe("memoryRevision", () => {
  const stored = memory("1", { content: "old", confidence: 0.5 });

  it("sends nothing when nothing changed", () => {
    expect(memoryRevision(stored, "old", 0.5)).toBeNull();
  });

  it("refuses empty content rather than forgetting by another name", () => {
    expect(memoryRevision(stored, "   ", 0.9)).toBeNull();
  });

  it("sends the confidence only when it changed", () => {
    expect(memoryRevision(stored, "new", 0.5)).toEqual({ content: "new", confidence: undefined });
    expect(memoryRevision(stored, "old", 0.75)).toEqual({ content: "old", confidence: 0.75 });
  });
});

describe("shortName", () => {
  it("keeps a short text whole, trimmed", () => {
    expect(shortName("  Draft follow-ups.  ")).toBe("Draft follow-ups.");
  });

  it("cuts a long text to the limit, ellipsis included", () => {
    const long = "Globex asks that all contact exports be sent to their new address.";
    const cut = shortName(long);
    expect(cut.length).toBeLessThanOrEqual(NAMED_CHARS);
    expect(cut.endsWith("…")).toBe(true);
    expect(long.startsWith(cut.slice(0, -1))).toBe(true);
  });
});
