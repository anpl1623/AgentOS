/**
 * The stylesheets' written promises, checked against the rules that make them.
 *
 * tokens.css carries two tables of measured contrast ratios, one per theme. A
 * table that drifts from its tokens is worse than none, so each row is
 * recomputed here from the token values: WCAG 2 relative luminance, with each
 * translucent layer composited over the one beneath it first, exactly as the
 * file's header describes. The dark theme is the first `:root` block; the
 * light theme is that block with `:root[data-theme="light"]` laid over it,
 * which is how the cascade resolves it on the page.
 */

import { describe, expect, it } from "vitest";

import layout from "./layout.css?raw";
import tokens from "./tokens.css?raw";

type Rgb = readonly [number, number, number];
type Properties = ReadonlyMap<string, string>;

/** The custom properties declared in the block that `pattern` captures. */
function declarations(css: string, pattern: RegExp): Map<string, string> {
  const body = pattern.exec(css)?.[1] ?? "";
  const values = new Map<string, string>();
  for (const match of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    values.set(match[1] ?? "", (match[2] ?? "").trim());
  }
  return values;
}

const DARK_BLOCK = /:root\s*\{([\s\S]*?)\n\}/;
const LIGHT_BLOCK = /:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/;

const darkOnly = declarations(tokens, DARK_BLOCK);
const lightOnly = declarations(tokens, LIGHT_BLOCK);

const THEMES: Record<"dark" | "light", Properties> = {
  dark: darkOnly,
  light: new Map([...darkOnly, ...lightOnly]),
};

/** A token as a colour, with its alpha. Follows `var()` references. */
function colour(properties: Properties, token: string): { rgb: Rgb; alpha: number } {
  const raw = properties.get(token);
  if (raw === undefined) throw new Error(`${token} is not defined`);
  const reference = /^var\((--[\w-]+)\)$/.exec(raw);
  if (reference?.[1]) return colour(properties, reference[1]);
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

/**
 * A ground as written in the table: a token, or layers `over` one another,
 * the last being the opaque ground they all sit on. Composited from the bottom
 * up, so `--glass over --aurora-1 over --surface-canvas` is the glass laid on
 * the aurora laid on the canvas.
 */
function ground(properties: Properties, written: string): Rgb {
  const layers = written.split(/\s+over\s+/).reverse();
  let below = colour(properties, layers[0] ?? "").rgb;
  for (const layer of layers.slice(1)) {
    const above = colour(properties, layer);
    below = above.rgb.map(
      (channel, index) => channel * above.alpha + (below[index] ?? 0) * (1 - above.alpha),
    ) as unknown as Rgb;
  }
  return below;
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

function measure(properties: Properties, token: string, written: string): number {
  return contrast(colour(properties, token).rgb, ground(properties, written));
}

interface Row {
  token: string;
  ground: string;
  stated: number;
  need: number;
}

const GROUND = String.raw`--[\w-]+(?:\s+over\s+--[\w-]+)*`;

function rows(text: string): Row[] {
  const pattern = new RegExp(
    String.raw`^ \*\s+(--[\w-]+)\s+(${GROUND})[^\n]*?(\d+\.\d\d)\s+(\d\.\d)\s*$`,
    "gm",
  );
  return [...text.matchAll(pattern)].map((match) => ({
    token: match[1] ?? "",
    ground: match[2] ?? "",
    stated: Number(match[3]),
    need: Number(match[4]),
  }));
}

/**
 * The figures quoted in a table's prose, written as `<token> on <ground>
 * measures <ratio>`. They are held to the tokens as strictly as the rows.
 */
function quoted(text: string): { token: string; ground: string; stated: number }[] {
  const prose = text.replace(/\n \*\s*/g, " ");
  const pattern = new RegExp(String.raw`(--[\w-]+) on (${GROUND}) measures (\d+\.\d\d)`, "g");
  return [...prose.matchAll(pattern)].map((match) => ({
    token: match[1] ?? "",
    ground: match[2] ?? "",
    stated: Number(match[3]),
  }));
}

// The dark table is the file's header, before the first block; the light
// table is the comment between the dark block and the light one.
const darkStart = tokens.search(/^:root\s*\{/m);
const darkEnd = tokens.indexOf("\n}", darkStart);
const lightStart = tokens.search(/^:root\[data-theme="light"\]/m);
const TABLES = {
  dark: tokens.slice(0, darkStart),
  light: tokens.slice(darkEnd, lightStart),
} as const;

describe.each(["dark", "light"] as const)("the %s contrast table in tokens.css", (theme) => {
  const properties = THEMES[theme];
  const table = rows(TABLES[theme]);

  it("has rows to check", () => {
    expect(table.length).toBeGreaterThan(25);
  });

  it.each(table)("$token on $ground measures $stated and clears $need", (row) => {
    const measured = measure(properties, row.token, row.ground);
    expect(measured.toFixed(2)).toBe(row.stated.toFixed(2));
    expect(measured).toBeGreaterThanOrEqual(row.need);
  });

  it("quotes figures in its notes that match the tokens", () => {
    const figures = quoted(TABLES[theme]);
    expect(figures.length).toBeGreaterThan(0);
    for (const figure of figures) {
      const measured = measure(properties, figure.token, figure.ground);
      expect(measured.toFixed(2), `${figure.token} on ${figure.ground}`).toBe(
        figure.stated.toFixed(2),
      );
    }
  });

  it("measures white text on both ends of the gradient, resting and hovered", () => {
    const grounds = table.filter((row) => row.token === "--on-accent").map((row) => row.ground);
    expect(grounds).toEqual(
      expect.arrayContaining(["--accent", "--accent-2", "--accent-hover", "--accent-2-hover"]),
    );
  });
});

describe("the dark theme's notes", () => {
  const dark = THEMES.dark;

  it("is right to keep faint text off the accent wash", () => {
    expect(measure(dark, "--text-faint", "--accent-wash over --surface-panel")).toBeLessThan(4.5);
  });

  it("is right to keep faint text off a hovered control", () => {
    expect(measure(dark, "--text-faint", "--surface-hover")).toBeLessThan(4.5);
  });
});

describe("the light theme", () => {
  // Geometry, type and motion belong to both themes. A light block that
  // redefined one would make the two themes lay out differently.
  const SHARED = /^--(s-\d|fs-|lh-|fw-|radius-|dur-|ease-|z-|motion-|font-|tracking-|chart-row|chart-gap|sidebar-w|rail-w|topbar-h|content-max|gutter|glass-blur|stagger-step|delegation-)/;

  it("redefines colours only", () => {
    for (const name of lightOnly.keys()) {
      expect(name, `${name} is not a colour`).not.toMatch(SHARED);
      expect(darkOnly.has(name), `${name} has no dark value`).toBe(true);
    }
  });

  it("redefines every colour the dark theme writes as a literal", () => {
    // A token holding a literal that the light block forgot would carry a
    // dark-theme colour onto a white panel. References (`var()`) follow the
    // theme on their own and need no second value.
    const literal = /#[0-9a-f]{3,8}\b|rgba?\(/i;
    const missing = [...darkOnly]
      .filter(([, value]) => literal.test(value))
      .map(([name]) => name)
      .filter((name) => !lightOnly.has(name));
    expect(missing).toEqual([]);
  });

  it("is what the window paints before the theme module has run", () => {
    const early = declarations(
      tokens,
      /:root:not\(\[data-theme\]\)\s*\{([\s\S]*?)\n\s*\}/,
    );
    expect(early.size).toBeGreaterThan(0);
    for (const [name, value] of early) {
      expect(value, name).toBe(lightOnly.get(name));
    }
  });
});

describe("reduced motion", () => {
  it("stops every duration token", () => {
    const reduced = declarations(
      tokens,
      /@media \(prefers-reduced-motion: reduce\)\s*\{\s*:root\s*\{([\s\S]*?)\n\s*\}/,
    );
    const durations = [...darkOnly.keys()].filter((name) => /^--(dur-|motion-(fast|base))/.test(name));
    expect(durations.length).toBeGreaterThanOrEqual(4);
    for (const name of durations) {
      expect(reduced.get(name), name).toBe("0.01ms");
    }
    expect(reduced.get("--stagger-step")).toBe("0ms");
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
