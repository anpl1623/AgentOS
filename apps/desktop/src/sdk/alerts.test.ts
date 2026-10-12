import { describe, expect, it } from "vitest";

import components from "../styles/components.css?raw";
import {
  ALERT_CAP,
  ALERT_LEVELS,
  type Alert,
  type AlertInput,
  toAlert,
  withAlert,
  withoutAlert,
} from "./alerts";

function alert(id: string, level: AlertInput["level"] = "info", sticky = false): Alert {
  return toAlert({ id, level, message: id, sticky }, id);
}

function raiseAll(...alerts: Alert[]): Alert[] {
  return alerts.reduce<Alert[]>((list, each) => withAlert(list, each), []);
}

function ids(list: readonly Alert[]): string[] {
  return list.map((each) => each.id);
}

describe("raising by id", () => {
  it("replaces an alert with the same id in place instead of stacking", () => {
    const list = raiseAll(alert("a"), alert("b"), { ...alert("a"), message: "again" });
    expect(ids(list)).toEqual(["a", "b"]);
    expect(list[0]?.message).toBe("again");
  });

  it("gives an alert without an id the fallback", () => {
    expect(toAlert({ level: "info", message: "x" }, "alert-9").id).toBe("alert-9");
  });

  it("removes by id", () => {
    expect(ids(withoutAlert(raiseAll(alert("a"), alert("b")), "a"))).toEqual(["b"]);
  });
});

describe("the cap", () => {
  it("drops the oldest non-sticky alert when full", () => {
    const list = raiseAll(alert("1"), alert("2"), alert("3"), alert("4"), alert("5"));
    expect(ALERT_CAP).toBe(4);
    expect(ids(list)).toEqual(["2", "3", "4", "5"]);
  });

  it("drops a non-sticky alert before an older sticky one", () => {
    const list = raiseAll(
      alert("sticky", "warn", true),
      alert("2"),
      alert("3"),
      alert("4"),
      alert("5"),
    );
    expect(ids(list)).toEqual(["sticky", "3", "4", "5"]);
  });

  it("still shows a new alert when every alert on screen is sticky", () => {
    const list = raiseAll(
      alert("s1", "warn", true),
      alert("s2", "warn", true),
      alert("s3", "warn", true),
      alert("s4", "warn", true),
      alert("new"),
    );
    expect(ids(list)).toEqual(["s1", "s2", "s3", "s4", "new"]);
  });

  it("never evicts the alert being raised to make room for itself", () => {
    const list = raiseAll(
      alert("s1", "warn", true),
      alert("s2", "warn", true),
      alert("s3", "warn", true),
      alert("s4", "warn", true),
      alert("old"),
      alert("new"),
    );
    expect(ids(list)).toEqual(["s1", "s2", "s3", "s4", "new"]);
  });

  it("replacing an alert does not evict anything", () => {
    const full = raiseAll(alert("1"), alert("2"), alert("3"), alert("4"));
    expect(ids(withAlert(full, { ...alert("2"), message: "updated" }))).toEqual([
      "1",
      "2",
      "3",
      "4",
    ]);
  });
});

describe("errors", () => {
  it("are always sticky, whatever the caller asked", () => {
    expect(toAlert({ level: "error", message: "broken", sticky: false }, "e").sticky).toBe(true);
    expect(toAlert({ level: "warn", message: "w" }, "w").sticky).toBe(false);
  });

  it("are never dropped by the cap, however many arrive", () => {
    const errors = Array.from({ length: ALERT_CAP * 2 }, (_, n) => alert(`e${n}`, "error"));
    const list = raiseAll(...errors, alert("info"));
    expect(ids(list).filter((id) => id.startsWith("e"))).toEqual(errors.map((each) => each.id));
    expect(ids(list)).toContain("info");
  });

  it("survive a flood of ordinary alerts", () => {
    const flood = Array.from({ length: 20 }, (_, n) => alert(`i${n}`));
    const list = raiseAll(alert("broken", "error"), ...flood);
    expect(ids(list)).toContain("broken");
    expect(list).toHaveLength(ALERT_CAP);
  });
});

describe("levels", () => {
  it("each has a tone in the stylesheet that renders it", () => {
    // A level the stylesheet does not know renders as an untoned card: a
    // warning that looks like a note.
    for (const level of ALERT_LEVELS) {
      expect(components, `no .alert.${level} rule`).toMatch(
        new RegExp(`\\.alert\\.${level}\\s*\\{`),
      );
    }
  });
});
