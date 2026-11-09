import { describe, expect, it } from "vitest";

import { isIconName } from "../../components/icons";
import { SECTIONS, activeSection, section, stripScroll, visibleSections } from "./sectionsModel";

describe("SECTIONS", () => {
  it("starts with appearance and gives every section a distinct id and icon", () => {
    expect(SECTIONS[0]!.id).toBe("settings-appearance");
    expect(new Set(SECTIONS.map((entry) => entry.id)).size).toBe(SECTIONS.length);
    expect(new Set(SECTIONS.map((entry) => entry.icon)).size).toBe(SECTIONS.length);
    for (const entry of SECTIONS) expect(isIconName(entry.icon)).toBe(true);
  });

  it("finds a section by id, and refuses one that is not on the screen", () => {
    expect(section("settings-audit").label).toBe("Audit log");
    expect(() => section("settings-nowhere")).toThrow();
  });
});

describe("visibleSections", () => {
  it("keeps every section once the settings are read", () => {
    expect(visibleSections(true)).toEqual(SECTIONS);
  });

  it("drops the sections drawn from the settings while they cannot be read", () => {
    expect(visibleSections(false).map((entry) => entry.id)).toEqual([
      "settings-appearance",
      "settings-integrations",
      "settings-credentials",
      "settings-scheduler",
      "settings-audit",
    ]);
  });
});

describe("activeSection", () => {
  const tops = [
    { id: "a", top: -400 },
    { id: "b", top: 60 },
    { id: "c", top: 220 },
    { id: "d", top: 900 },
  ];

  it("marks the last section whose heading has reached the reading line", () => {
    expect(activeSection(tops, 90, false)).toBe("b");
    expect(activeSection(tops, 230, false)).toBe("c");
  });

  it("marks a short section jumped to, though the next heading is in view", () => {
    // A jump puts b's heading at 60, just above the line; c is on screen but
    // has not reached the line, so the section asked for is the one marked.
    expect(activeSection(tops, 84, false)).toBe("b");
  });

  it("marks the first section above the first heading, and none when there are none", () => {
    expect(activeSection([{ id: "a", top: 300 }, { id: "b", top: 900 }], 80, false)).toBe("a");
    expect(activeSection([], 80, false)).toBeNull();
  });

  it("marks the last section at the foot of the page, wherever its heading is", () => {
    expect(activeSection(tops, 90, true)).toBe("d");
  });
});

describe("stripScroll", () => {
  it("leaves the strip alone when the chip is already in view", () => {
    expect(stripScroll(120, 80, 100, 400, 16)).toBe(100);
  });

  it("scrolls back just far enough for a chip off the leading edge", () => {
    expect(stripScroll(60, 80, 100, 400, 16)).toBe(44);
    expect(stripScroll(4, 80, 100, 400, 16)).toBe(0);
  });

  it("scrolls on just far enough for a chip off the trailing edge", () => {
    expect(stripScroll(480, 90, 100, 400, 16)).toBe(186);
  });
});
