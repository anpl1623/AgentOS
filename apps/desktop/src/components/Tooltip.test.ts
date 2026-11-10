import { describe, expect, it } from "vitest";

import { tip } from "./Tooltip";

describe("tip", () => {
  it("names the element with the same words its tooltip shows", () => {
    const attributes = tip("Copy the run id");
    expect(attributes["aria-label"]).toBe("Copy the run id");
    expect(attributes["data-tip"]).toBe("Copy the run id");
  });

  it("writes no side or alignment for the defaults the stylesheet assumes", () => {
    expect(tip("Copy")).toEqual({ "aria-label": "Copy", "data-tip": "Copy" });
  });

  it("states a side other than below, and an end alignment", () => {
    expect(tip("Collapse", "right")).toEqual({
      "aria-label": "Collapse",
      "data-tip": "Collapse",
      "data-tip-side": "right",
    });
    expect(tip("Copy path", "top", "end")).toEqual({
      "aria-label": "Copy path",
      "data-tip": "Copy path",
      "data-tip-side": "top",
      "data-tip-align": "end",
    });
  });
});
