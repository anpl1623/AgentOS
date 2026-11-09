import { describe, expect, it } from "vitest";

import { NAV } from "../shell/nav";
import { ICON_NAMES, isIconName } from "./icons";

describe("the icon registry", () => {
  it("names each icon once", () => {
    expect(new Set(ICON_NAMES).size).toBe(ICON_NAMES.length);
  });

  it("has a drawing for every name it lists", () => {
    for (const name of ICON_NAMES) expect(isIconName(name), name).toBe(true);
  });

  it("does not claim names it has no drawing for", () => {
    expect(isIconName("toString")).toBe(false);
    expect(isIconName("")).toBe(false);
  });

  it("draws every screen in the sidebar, each differently", () => {
    for (const item of NAV) expect(isIconName(item.icon), item.route).toBe(true);
    expect(new Set(NAV.map((item) => item.icon)).size).toBe(NAV.length);
  });
});
