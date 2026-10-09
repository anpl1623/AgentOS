/**
 * The stylesheets' written promises, checked against the rules that make them.
 *
 * tokens.css carries a table of measured contrast ratios. A table that drifts
 * from its tokens is worse than none, so each row is recomputed here from the
 * token values: WCAG 2 relative luminance, with each translucent wash
 * composited over its ground first, exactly as the table's header describes.
 */

import { describe, expect, it } from "vitest";

import layout from "./layout.css?raw";
import tokens from "./tokens.css?raw";

type Rgb = readonly [number, number, number];

/** The first `:root` block's custom properties: the theme the app ships. */
function customProperties(css: string): Map<string, string> {
  const root = /:root\s*\{([\s\S]*?)\n\}/.exec(css)?.[1] ?? "";
  const values = new Map<string, string>();
  for (const match of root.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    values.set(match[1] ?? "", (match[2] ?? "").trim());
  }
  return values;
}

const properties = customProperties(tokens);

/** A token as a colour, with its alpha. Follows `var()` references. */
function colour(token: string): { rgb: Rgb; alpha: number } {
  const raw = properties.get(token);
  if (raw === undefined) throw new Error(`${token} is not defined`);
  const reference = /^var\((--[\w-]+)\)$/.exec(raw);
  if (reference?.[1]) return colour(reference[1]);
  const hex = /^#([0-9a-f]{6})$/i.exec(raw);
  if (hex?.[1]) {
    const value = hex[1];
    return {
      rgb: [0, 2, 4].map((at) => Number.parseInt(value.slice(at, at + 2), 16)) as unknown as Rgb,
      alpha: 1,
    };
  }
  const rgba = /^rgba\((\d+),\s*(\d+),\s*(\d+),\s*([\d.]+)\)$/.exec(raw);
  if (rgba) {
    return {
      rgb: [Number(rgba[1]), Number(rgba[2]), Number(rgba[3])],
      alpha: Number(rgba[4]),
    };
  }
  throw new Error(`${token} is \`${raw}\`, which this test cannot read`);
}

/** A ground as written in the table: a token, or a wash `over` a token. */
function ground(written: string): Rgb {
  const [top, under] = written.split(/\s+over\s+/);
  const base = colour(under ?? top ?? "");
  if (under === undefined) return base.rgb;
  const wash = colour(top ?? "");
  return wash.rgb.map(
    (channel, index) => channel * wash.alpha + (base.rgb[index] ?? 0) * (1 - wash.alpha),
  ) as unknown as Rgb;
}

function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map((channel) => {
    const v = channel / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  }) as unknown as Rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: Rgb, b: Rgb): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (high + 0.05) / (low + 0.05);
}

interface Row {
  token: string;
  ground: string;
  stated: number;
  need: number;
}

const ROWS: Row[] = [
  ...tokens.matchAll(/^ \*\s+(--[\w-]+)\s+(--[\w-]+(?:\s+over\s+--[\w-]+)?)[^\n]*?(\d+\.\d\d)\s+(\d\.\d)\s*$/gm),
].map((match) => ({
  token: match[1] ?? "",
  ground: match[2] ?? "",
  stated: Number(match[3]),
  need: Number(match[4]),
}));

describe("the contrast table in tokens.css", () => {
  it("has rows to check", () => {
    expect(ROWS.length).toBeGreaterThan(20);
  });

  it.each(ROWS)("$token on $ground measures $stated and clears $need", (row) => {
    const measured = contrast(colour(row.token).rgb, ground(row.ground));
    expect(measured.toFixed(2)).toBe(row.stated.toFixed(2));
    expect(measured).toBeGreaterThanOrEqual(row.need);
  });

  it("is right to keep faint text off the accent wash", () => {
    const faint = contrast(
      colour("--text-faint").rgb,
      ground("--accent-wash over --surface-panel"),
    );
    expect(faint).toBeLessThan(4.5);
  });
});

/** The declarations of the rule whose selector is exactly `selector`. */
function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?:^|\\n)${escaped}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";
}

describe("the shell's scroll columns", () => {
  // Every status chip carries an absolutely positioned `.visually-hidden`
  // label. A column that is not a containing block lets those labels escape
  // it and lengthen the document, so the whole window scrolls.
  it.each([".main", ".sidebar"])("%s is a containing block", (selector) => {
    expect(rule(layout, selector)).toMatch(/position:\s*relative/);
  });
});

describe("reflow at 200% zoom", () => {
  // The minimum window is 940 by 620, which is 470 by 310 at 200%.
  it("lets a row's chips wrap below its title instead of drawing over it", () => {
    expect(rule(layout, ".row")).toMatch(/flex-wrap:\s*wrap/);
    expect(rule(layout, ".row-meta > *")).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("lets the sidebar give way rather than holding its widest size", () => {
    // `minmax(a, b)` beside `1fr` always resolves to `b`.
    expect(rule(layout, ".shell")).not.toMatch(/minmax\(/);
  });
});
