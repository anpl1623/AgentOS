import { describe, expect, it } from "vitest";

import { type KeyPress, commandFor, isApple, shortcutLines } from "./keys";
import { NAV } from "./nav";

function press(key: string, extra: Partial<KeyPress> = {}): KeyPress {
  return {
    key,
    code: /^[0-9]$/.test(key) ? `Digit${key}` : "",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...extra,
  };
}

const cmd = { metaKey: true };
const ctrl = { ctrlKey: true };

describe("commandFor", () => {
  it("maps Cmd or Ctrl with 1 to 7 onto the screens in sidebar order", () => {
    expect(NAV).toHaveLength(7);
    NAV.forEach((item, index) => {
      for (const mod of [cmd, ctrl]) {
        expect(commandFor(press(String(index + 1), mod), false)).toEqual({
          kind: "screen",
          route: item.route,
        });
      }
    });
    expect(commandFor(press("8", cmd), false)).toBeNull();
  });

  it("reads the physical digit where the layout needs Shift for it", () => {
    expect(commandFor(press("&", { ...cmd, code: "Digit1" }), false)).toEqual({
      kind: "screen",
      route: "dashboard",
    });
  });

  it("binds no bare letter and no bare digit", () => {
    for (const key of [..."abcdefghijklmnopqrstuvwxyz", ..."0123456789"]) {
      expect(commandFor(press(key), false)).toBeNull();
      expect(commandFor(press(key.toUpperCase(), { shiftKey: true }), false)).toBeNull();
    }
  });

  it("leaves the keys an approval card might read as a decision unbound", () => {
    // The shell can only navigate, but a global binding on these keys would
    // still steal them from the approvals screen, which reads them itself.
    for (const key of ["Enter", " ", "a", "d", "y", "n"]) {
      for (const mod of [{}, cmd, ctrl, { altKey: true }, { shiftKey: true }]) {
        expect(commandFor(press(key, mod), false)).toBeNull();
      }
    }
  });

  it("ignores everything while typing except Cmd or Ctrl with comma", () => {
    expect(commandFor(press("1", cmd), true)).toBeNull();
    expect(commandFor(press("k", cmd), true)).toBeNull();
    expect(commandFor(press("[", cmd), true)).toBeNull();
    expect(commandFor(press("ArrowLeft", { altKey: true }), true)).toBeNull();
    expect(commandFor(press("?", { shiftKey: true }), true)).toBeNull();
    expect(commandFor(press(",", cmd), true)).toEqual({ kind: "screen", route: "settings" });
    expect(commandFor(press(",", ctrl), true)).toEqual({ kind: "screen", route: "settings" });
  });

  it("opens the palette, goes back and forward, and shows the sheet", () => {
    expect(commandFor(press("k", cmd), false)).toEqual({ kind: "palette" });
    expect(commandFor(press("K", ctrl), false)).toEqual({ kind: "palette" });
    expect(commandFor(press("[", cmd), false)).toEqual({ kind: "back" });
    expect(commandFor(press("]", cmd), false)).toEqual({ kind: "forward" });
    expect(commandFor(press("ArrowLeft", { altKey: true }), false)).toEqual({ kind: "back" });
    expect(commandFor(press("ArrowRight", { altKey: true }), false)).toEqual({ kind: "forward" });
    expect(commandFor(press("?", { shiftKey: true }), false)).toEqual({ kind: "shortcuts" });
  });

  it("leaves combinations it does not own to the platform", () => {
    expect(commandFor(press("k", { ...cmd, shiftKey: true }), false)).toBeNull();
    expect(commandFor(press("1", { ...cmd, altKey: true }), false)).toBeNull();
    expect(commandFor(press("ArrowLeft"), false)).toBeNull();
    expect(commandFor(press("ArrowLeft", cmd), false)).toBeNull();
    expect(commandFor(press("Escape"), false)).toBeNull();
    expect(commandFor(press("?", cmd), false)).toBeNull();
  });
});

describe("shortcutLines", () => {
  it("lists a line for every screen and names the platform's modifier", () => {
    const apple = shortcutLines(true);
    const other = shortcutLines(false);
    for (const item of NAV) {
      expect(apple.some((line) => line.does === `Go to ${item.label}`)).toBe(true);
    }
    expect(apple[0]?.keys[0]).toEqual(["⌘", "1"]);
    expect(other[0]?.keys[0]).toEqual(["Ctrl", "1"]);
  });

  it("recognises Apple platforms", () => {
    expect(isApple("MacIntel")).toBe(true);
    expect(isApple("iPad")).toBe(true);
    expect(isApple("Win32")).toBe(false);
    expect(isApple("Linux x86_64")).toBe(false);
  });
});
