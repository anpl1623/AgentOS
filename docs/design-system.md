# Design system

This document describes how the AgentOS desktop app looks, moves and responds, and why. It covers
the tokens, the two themes, the shell's three layouts, the motion vocabulary, the shared primitives
and the rules a screen owner follows. It reflects what is built in `apps/desktop/src` today; where a
piece exists but is not yet used by any screen, it says so.

## Purpose and principles

The desktop app is where a person supervises agents that act on their machine. Most of what it shows
is a list of facts: what an agent did, what it was allowed to do, what it was refused, and what is
waiting on a decision. The design follows from that.

**A calm control room.** Deep ink grounds, raised cards, chrome drawn in glass over a faint aurora.
Nothing on a resting screen competes for attention, so that when something does, it is noticed.

**Loud only for risk, taint and decisions.** Light, colour and glow are spent on three things: how
much harm an action could do (the risk chips, with high and critical glowing), whether a run has read
untrusted data (the taint mark), and anything waiting on a person (the pending approval card and the
approvals count, both in the attention hue). A glow that is always on stops meaning anything.

**Motion that means something.** A screen arriving rises into place; a list arrives in reading
order; a number that changed pops once; a live thing pulses for as long as it is live; an overlay
springs in from where it was summoned. Nothing loops that is not live, and nothing moves for
decoration. A person who has asked their system for less motion gets none.

Four further rules hold everywhere:

1. **Colour is never the only signal.** Every tone also has a shape: a banner's leading bar, a
   refusal's leading bar, the dashed edge of a cancelling run, the hatch in a chart, the arrow on a
   delta, the `⚠` before a taint label.
2. **Contrast is measured, not judged.** Every text and control colour is recomputed by a test from
   the token values. See [Tokens](#tokens).
3. **Geometry belongs to the stylesheet, state to the script.** The layout is chosen by media and
   container queries, so the first paint is right and a resize never waits on a render. Script holds
   only what a stylesheet cannot know.
4. **Approving costs a deliberate click.** No key binding and no palette entry can resolve an
   approval (`shell/keys.ts`, `shell/palette.ts`). The palette goes places; it never does things.

## Where it lives

`src/styles.css` is the manifest. It only fixes the order the sheets load in, and the order is
load-bearing:

| Order | File | Holds |
| --- | --- | --- |
| 1 | `styles/fonts.css` | Nothing, on purpose: the app uses the system stack. A vendored face would be declared here. |
| 2 | `styles/tokens.css` | Every colour literal in the app, both themes, the contrast tables, and the reduced-motion overrides. |
| 3 | `styles/base.css` | Bare elements: the document, the aurora, headings, `kbd`, buttons, fields. |
| 4 | `styles/motion.css` | Keyframes, the motion utilities, and the theme cross-fade. |
| 5 | `styles/layout.css` | The shell, the three layouts, the page frame, panels, grids and rows. |
| 6 | `styles/components.css` | The approval card, trace, feed, banners, skeletons, overlays and alerts. |
| 7 | `styles/primitives.css` | Icons, tooltips, the spotlight and lift, and the primitives in `components/`. |
| 8 | `styles/status.css` | The `.risk` and `.verdict` chips. |
| 9 | `styles/charts.css` | Figures, tracks, bars, legends and the chart tip. |
| 10 | `styles/screens/<screen>.css` | One sheet per screen, for rules no other screen uses. |
| 11 | `styles/focus.css` | The focus ring, last, so no rule loaded later can remove it. |

The theme logic is `src/sdk/theme.ts`. The shell is `src/shell/`. The primitives are in
`src/components/`, and screens import them through `components/common.tsx`.

## Themes

There are two themes, dark and light. Dark is the home theme: `:root` in `tokens.css` is the dark
theme, and `:root[data-theme="light"]` redefines its colours.

A person chooses `system` (the default), `dark` or `light`. The choice is kept in `localStorage`
under `agentos.theme`. The theme it resolves to is written to `<html>` as `data-theme="dark|light"`,
which `tokens.css` reads, and as `style.colorScheme`, which native controls and scrollbars read.

```ts
import {
  type ThemePreference,   // "system" | "dark" | "light"
  type ResolvedTheme,     // "dark" | "light"
  THEME_PREFERENCES,      // ["system", "dark", "light"], in the order a control offers them
  THEME_STORAGE_KEY,      // "agentos.theme"
  THEME_CROSSFADE_MS,     // 300, the same figure as --dur-theme
  isThemePreference,      // (value) => value is ThemePreference
  parseThemePreference,   // (raw) => ThemePreference; anything unknown means "system"
  resolveTheme,           // (preference, systemPrefersDark) => ResolvedTheme
  nextThemePreference,    // system → dark → light → system
  readThemePreference,    // reads storage inside try/catch; "system" on any failure
  applyTheme,             // main.tsx calls it once, before createRoot; never cross-fades
  setThemePreference,     // remember, show and publish a choice, from outside React
  currentTheme,           // { preference, resolved }, from outside React
  useTheme,               // { preference, resolved, setPreference }
} from "../sdk/theme";
```

The decisions (`parseThemePreference`, `resolveTheme`, `nextThemePreference`) are pure and tested in
`sdk/theme.test.ts`. The store is a module rather than a React context, so code outside React can
change the theme without a provider.

How it behaves:

- **No flash on load.** `main.tsx` calls `applyTheme()` before React's first render. The module is
  deferred, so for the frame before it runs `tokens.css` has
  `@media (prefers-color-scheme: light) { :root:not([data-theme]) { … } }`, which paints the light
  page ground on a light desktop. A test holds those values to the light theme's own.
  `index.html` declares `<meta name="color-scheme" content="dark light">` so the browser's own
  canvas follows the system too. An inline boot script is not an option: the Tauri CSP
  (`default-src 'self'`) does not allow inline scripts.
- **Following the system.** While the preference is `system`, a `matchMedia` change listener
  re-resolves the theme as soon as the operating system switches.
- **Other windows.** A `storage` event for `agentos.theme`, or storage being cleared, re-reads the
  preference.
- **Cross-fade.** A change of theme, but never the initial load, adds `theme-switching` to `<html>`
  for `THEME_CROSSFADE_MS` plus 50ms. While it is present, `motion.css` transitions background,
  text, border, outline, fill, stroke and shadow colours, and nothing else. The class is not added
  under reduced motion, or when the resolved theme does not change.
- **Failure.** Storage that throws or is missing never breaks start-up. The choice then applies to
  the window without being remembered, and a system that cannot report its scheme is treated as
  dark.

The sidebar's `ThemeToggle` (`shell/ThemeToggle.tsx`, names and icons in `shell/themeChoices.ts`) is
the only control that sets the theme today.

README screenshots are taken in a chosen theme regardless of the capturing machine:
`npm run screenshots -- --out <dir> --size 1500x950 --theme dark|light`. The default is `dark`.

## Tokens

Every token is a custom property on `:root` in `styles/tokens.css`.

### Rules

- **`tokens.css` is the only file that may contain a colour literal.** Hex values, `rgba()`, and
  colours inside shadows and gradients all count. Every other sheet uses `var(--token)`. The
  keywords `transparent` and `currentColor` are allowed anywhere. No test enforces this yet; at the
  time of writing it holds, and
  `grep -rnE "#[0-9a-fA-F]{3,8}\b|rgba?\(" src/styles --include='*.css'` should only ever match
  `tokens.css`.
- **Every colour is measured.** `tokens.css` has two contrast tables, one per theme. Each row names a
  token, the worst ground it can land on, the ratio to two decimals, and what it needs: text needs
  4.5:1, and a control's edge and the focus ring need 3:1. `styles/stylesheets.test.ts` recomputes
  every row with WCAG 2 relative luminance, and also checks any figure quoted in a table's notes in
  the form "`<token>` on `<ground>` measures `<ratio>`". After changing a colour, run
  `npx vitest run src/styles` from `apps/desktop`. A failing row prints what it measured, so the
  table can be corrected from the output. Never change a figure by hand to make a row pass. The
  figures are not repeated in this document, because a copy here would not be tested.
- **Grounds compose from the bottom up.** A ground written `--glass over --aurora-1 over
  --surface-canvas` is the glass laid on the aurora laid on the canvas. The test composites any
  number of translucent layers before measuring.
- **The light theme redefines colours only.** The test fails if `:root[data-theme="light"]` sets
  spacing, type, radius, motion, z-index or frame geometry, if it introduces a token the dark theme
  does not have, or if a dark token holding a colour literal has no light value. A token that only
  refers to others, such as `--chart-ink: var(--text-dim)`, follows the theme on its own.
- **Faint text stays off two grounds**: a hovered control (`--surface-hover`) and any wash
  (`--accent-wash` and the status washes). Use `--text-dim` there. Hovered rows use
  `--surface-raised` for this reason. A test checks that the dark theme really does fall short on
  both grounds, so the rule stays justified.
- **`--danger-fg` is for edges and marks only.** Words in the danger hue use `--danger-text`.
- **Status glows are for loud moments only**: a pending decision, a high or critical risk chip, a
  stat tile in an attention or failure tone, the approvals count, a hovered danger button. The accent
  glow marks where the person is or what they are about to press: the current nav item's bar, the
  brand tile, the active palette entry, a focused approval card, a hovered primary button.

### Grounds

From the darkest to the lightest. Every text token is measured against all of them.

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--surface-nav` | `#080c13` | `#eef1f6` | The opaque ground of the sidebar and top bar where glass is unsupported |
| `--surface-sunken` | `#070b11` | `#f2f4f8` | Wells: fields, code, tags, chart lanes, the switch track |
| `--surface-base` | `#0a0f17` | `#f4f6fa` | The page ground |
| `--surface-canvas` | `var(--surface-base)` | follows | The ground under the aurora; `body` uses it |
| `--surface-recessed` | `#0d131d` | `#f0f3f8` | A step above the page; the approval footer's opaque fallback |
| `--surface-panel` | `#0f1621` | `#ffffff` | Cards and panels |
| `--surface-raised` | `#151d2a` | `#f7f9fc` | Raised headers, resting buttons, hovered rows |
| `--surface-hover` | `#1c2739` | `#e9eef5` | Hovered controls. No faint text on it |

### Aurora and glass

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--aurora-1` | `rgba(47,109,224,.16)` | `rgba(47,109,224,.10)` | Accent glow, about a third of the way across, above the top |
| `--aurora-2` | `rgba(110,82,240,.12)` | `rgba(106,79,232,.08)` | Violet glow, about four fifths of the way across |
| `--glass` | `rgba(11,16,25,.74)` | `rgba(250,251,253,.74)` | The sidebar, the top bar and a pinned page header |
| `--glass-strong` | `rgba(15,21,32,.90)` | `rgba(255,255,255,.90)` | Overlays: dialogs, the palette, alerts, the chart tip, the approval footer, the open drawer |
| `--glass-border` | `rgba(150,172,210,.12)` | `rgba(15,23,42,.09)` | The hairline edge of glass |
| `--glass-blur` | `16px` | shared | Used as `blur(var(--glass-blur)) saturate(140%)` |

The aurora is `body::before` in `base.css`: fixed, at `z-index: -1`, ignoring the pointer, two radial
gradients. It shows through glass and anything transparent, and opaque panels hide it, which keeps
every measurement on a panel true. The tables measure text on glass at the aurora's brightest point.

Glass is written the same way everywhere:

```css
.my-chrome {
  background: var(--glass);
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(140%);
  backdrop-filter: blur(var(--glass-blur)) saturate(140%);
  border: 1px solid var(--glass-border);
}
@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .my-chrome { background: var(--surface-nav); } /* the opaque ground it stands in for */
}
```

Always include the `-webkit-` line: older macOS WebKit, which Tauri uses, understands only the
prefixed form. The fallback exists because a busy screen showing through unblurred glass is noise,
even though the fill alone is what the tables measure.

### Borders

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--border-hairline` | `rgba(130,152,186,.10)` | `rgba(15,23,42,.06)` | Between the lines of the activity feed |
| `--border-subtle` | `#1a2433` | `#e2e7ef` | Between rows; the resting edge of panels and cards |
| `--border-strong` | `#263347` | `#d2d9e4` | Chips, tags, a lifted card's edge, scrollbars |
| `--border-emphasis` | `#34455e` | `#bcc6d5` | The one card that asks for a decision |
| `--border-control` | `#586b8b` | `#77849a` | The resting edge of anything interactive. Held to 3:1 |
| `--border-control-hover` | `#7689a8` | `#66758c` | The same edge under the pointer |

### Text

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--text` | `#e6ecf5` | `#0e1621` | Body text and titles |
| `--text-dim` | `#9ba8ba` | `#465366` | Secondary text, labels, section eyebrows; quiet text on a wash or a hovered control |
| `--text-faint` | `#7a889d` | `#5a6779` | The quietest tone: placeholders, timestamps, hints. Never on `--surface-hover` or a wash |
| `--text-accent` | `#cfe0ff` | `#1b4cae` | Text on the accent wash: the current nav item, a pressed toggle |
| `--on-accent` | `#ffffff` | `#ffffff` | Text and marks on the accent fill and gradient |
| `--on-warn` | `#1f1606` | `#ffffff` | The figure on the approvals count |

### Accent and focus

| Token | Value | Use |
| --- | --- | --- |
| `--accent` | `#2f6de0` in both themes | The solid accent; one end of the gradient |
| `--accent-hover` | `#2a60c8` | Hover darkens, so white text gains contrast at the moment it is aimed at |
| `--accent-2` | `#6a4fe8` | Violet, for the gradient only. Never a text colour |
| `--accent-2-hover` | `#5c42d2` | |
| `--accent-gradient` | `linear-gradient(135deg, var(--accent), var(--accent-2))` | The primary button, the nav marker's bar, the brand tile, a switch that is on, the active palette entry's icon |
| `--accent-gradient-hover` | the same, between the hover ends | A hovered primary button |
| `--accent-wash` | `rgba(47,109,224,.16)`, light `.10` | The fill of a chosen thing |
| `--accent-line` | `rgba(47,109,224,.35)` | The edge of a chosen thing; text selection |
| `--accent-glow` | a ring and a bloom, as a `box-shadow` | See the glow rule above |
| `--sheen` | `rgba(255,255,255,.10)` | The band of light that crosses a hovered primary button or a `.sheen` element |
| `--focus` | dark `#bcd4ff`, light `#0a1838` | The focus ring. At least 3:1 against both gradient ends and every ground |
| `--focus-glow` | translucent accent | The soft halo outside the ring. Decoration only |

### Status

Each hue has four roles. `fg` is for icons, outlines and small loud marks; `line` for borders; `wash`
for the one background tint the hue uses everywhere; `text` for sentences in that hue, desaturated so
a paragraph of it is readable. Four hues also have a `glow`: a complete `box-shadow` value, a
one-pixel ring plus a soft bloom, meant to be stacked after an elevation:
`box-shadow: var(--highlight-inset), var(--elev-2), var(--warn-glow);`.

| Hue | Means | `fg` dark / light | `text` dark / light | Glow |
| --- | --- | --- | --- | --- |
| `ok` | Succeeded, enabled, healthy | `#46b877` / `#13693a` | `#9bd8b3` / `#0f5530` | `--ok-glow` |
| `warn` | Attention: pending approvals, untrusted data, stale data, fixture data | `#d9a441` / `#8c5a00` | `#e8cd97` / `#6e4708` | `--warn-glow` |
| `danger` | A failure | `#e0644f` / `#c43a2a` (edges and marks only) | `#ef8878` / `#a62c1e` | `--danger-glow` |
| `critical` | The top risk step | `#ff7a91` / `#b3173f` | `#ff9aab` / `#991437` | `--critical-glow` |
| `info` | A run in flight; neutral information | `#7fb2ff` / `#2257c4` | `#a8c4ee` / `#1d479c` | none |
| `blocked` | A refusal by policy | `#b9cffa` / `#1b4cae` | none; `fg` carries the words | none |

Each hue's `line` and `wash` are in `tokens.css`, with darker values in the light theme. A refusal is
drawn on the accent hue as `blocked`, not as danger: a policy that refuses a call has done its job.
It is told apart from a live run by its opaque edge and the bar on its leading side.

### Depth

| Token | Use |
| --- | --- |
| `--highlight-inset` | A one-pixel line of light along the top edge of any raised surface: buttons, cards, `kbd` |
| `--shadow-well` | Inset shade at the top of a sunken well: fields, code, the segmented track, chart lanes |
| `--elev-1` | A resting card |
| `--elev-2` | A lifted or hovered card, a pending approval card, a pinned page header, tooltips, the chart tip |
| `--elev-3` | Overlays: dialogs, the palette, alerts, the open drawer |
| `--shadow-overlay` | An alias of `--elev-3`, kept for older rules |
| `--spotlight` | The colour of the pointer light on `[data-spotlight]` elements |
| `--scrim` | The dimmed backdrop behind overlays and the drawer |

### Charts

| Token | Value | Use |
| --- | --- | --- |
| `--chart-grid` | `#1a2433` / `#e2e7ef` | Grid lines |
| `--chart-ink` | `var(--text-dim)` | Labels and axes, in the same ink as the text beside them |
| `--chart-hatch` | translucent text colour | The hatch for "did not execute" and "waiting on you" |
| `--chart-row` | `22px` | Lane height, read by the chart component |
| `--chart-gap` | `6px` | Gap between lanes |

### Type

The font is the platform's system stack (`--font-sans`) with `--font-mono` for code. Nothing is
downloaded.

| Token | Value | Use |
| --- | --- | --- |
| `--fs-micro` | `10.5px` | Risk chips, key caps, palette group headings |
| `--fs-eyebrow` | `11px` | `h2` section labels, page and stat tile eyebrows |
| `--fs-meta` | `11.5px` | Row meta lines, tags, tooltips, verdict chips |
| `--fs-code` | `12px` | Monospace text, labels, `h3` |
| `--fs-small` | `12.5px` | Secondary text, banners, segmented controls |
| `--fs-body` | `13.5px` | Body text |
| `--fs-title` | `19px` | Dialog titles, the palette input |
| `--fs-hero` | `26px` | Page titles (`h1`) |
| `--fs-stat` | `28px` | Stat figures |
| `--lh-tight`, `--lh-code`, `--lh-body` | `1.2`, `1.5`, `1.55` | |
| `--fw-normal` … `--fw-bold` | `400`, medium `550`, semibold `600`, heading `650`, bold `700` | |
| `--tracking-tight` | `-0.02em` | Titles and figures |
| `--tracking-eyebrow` | `0.08em` | Uppercase eyebrows |

`h1` is the page title: hero size, heading weight, tight tracking. `h2` is a section eyebrow: 11px,
uppercase, semibold, tracked out, in `--text-dim`. `h3` ranks below it at body case. Set any number
that changes in `font-variant-numeric: tabular-nums`, so a count that ticks over does not shift
sideways.

### Spacing

One scale. Every padding, margin and gap reads a step from it, so a density layer could later remap
these and nothing else.

| `--s-1` | `--s-2` | `--s-3` | `--s-4` | `--s-5` | `--s-6` | `--s-7` | `--s-8` | `--s-9` | `--s-10` | `--s-11` |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2px | 4px | 6px | 8px | 10px | 12px | 14px | 16px | 20px | 24px | 40px |

`--delegation-indent` is `var(--s-9)`: how far each level of delegation steps in on a trace.

### Frame, shape and layers

| Token | Value | Use |
| --- | --- | --- |
| `--sidebar-w` | `248px` | The full sidebar |
| `--rail-w` | `68px` | The icon rail |
| `--topbar-h` | `56px` | The narrow layout's top bar |
| `--content-max` | `1180px` | The widest a screen's column runs |
| `--gutter` | `clamp(16px, 3vw, 40px)` | The screen's side padding, growing with the window |
| `--radius-sm` / `-md` / `-lg` | `4px` / `6px` / `8px` | Chips; controls and tags; entries and wells |
| `--radius-xl` | `12px` | Cards, panels, banners, figures, alerts |
| `--radius-2xl` | `16px` | Dialogs and the palette |
| `--radius-pill` | `999px` | Verdict chips, counts, the switch |

Layers, lowest first: `--z-base` 0, `--z-sticky` 10 (pinned headers, the approval footer, the chart
tip), `--z-chrome` 20 (the sidebar, the top bar), `--z-drawer` 90 (the drawer; its scrim one below),
`--z-alerts` 99 (alerts and tooltips), `--z-overlay` 100 (dialogs and the palette), `--z-skip` 101
(the skip link). The alert stack sits below overlays so an alert never covers a question being asked.

### Motion

| Token | Value | For |
| --- | --- | --- |
| `--dur-1` | `90ms` | A press |
| `--dur-2` | `160ms` | A hover; colour changes |
| `--dur-3` | `240ms` | An entrance: a screen, an overlay, the drawer |
| `--dur-4` | `420ms` | A flourish: a pop, an alert arriving, bars growing |
| `--dur-pulse` | `1600ms` | One live-dot pulse |
| `--dur-shimmer` | `1400ms` | One skeleton sweep |
| `--dur-theme` | `300ms` | The theme cross-fade |
| `--stagger-step` | `35ms` | The gap between staggered children |
| `--ease-out` | `cubic-bezier(0.16, 1, 0.3, 1)` | Things arriving |
| `--ease-spring` | `cubic-bezier(0.34, 1.56, 0.64, 1)` | Things that should feel physical: overlays, counts, knobs, alerts, the nav marker |
| `--ease-in-out` | `cubic-bezier(0.65, 0, 0.35, 1)` | Things moving from one place to another; the sheen and shimmer |

Under `prefers-reduced-motion: reduce`, every `--dur-*` and `--motion-*` duration becomes `0.01ms`
and `--stagger-step` becomes `0ms`. Durations go to near zero rather than zero so that
`transitionend` and `animationend` still fire for any code waiting on them. A test checks that every
duration token is covered.

### Declared but not read

These tokens exist in `tokens.css` but no rule reads them at present: `--brand-outer` and
`--brand-inner` (the brand tile uses `--accent-gradient` and `--on-accent`), `--fs-display`,
`--ok-glow`, `--z-base`, and the older `--motion-fast`, `--motion-base` and `--motion-ease`. New rules
use the `--dur-*` and `--ease-*` scale. Either use one of these deliberately or remove it with its
light value; do not add another.

## Layout modes and the shell

### The three layouts

`layout.css` chooses between three layouts with media queries. `shell/layout.ts` holds the same
breakpoints (`BREAKPOINTS = { compact: 720, wide: 1200 }`), and `shell/layout.test.ts` checks that
`layout.css` contains each query exactly as `mediaQuery(mode)` writes it. The upper bounds stop
0.02px short of the next mode, so a fractional width at an odd zoom level falls in exactly one.

| Mode | Width | Media query | Sidebar | Its button |
| --- | --- | --- | --- | --- |
| `wide` | 1200px and up | `(min-width: 1200px)` | A full column (`--sidebar-w`): icons, labels and counts | Collapses the column to the rail (`--rail-w`) and expands it again. The choice is kept in `localStorage["agentos.sidebar"]` as `"collapsed"` or `"expanded"` |
| `compact` | 720px to 1199px | `(min-width: 720px) and (max-width: 1199.98px)` | The icon rail, with labels as tooltips | Opens the full sidebar as a drawer over the screen. The column keeps the rail's width, so the screen does not move |
| `narrow` | under 720px | `(max-width: 719.98px)` | No column. A glass top bar (`--topbar-h`) and an off-canvas drawer from the left | The top bar's menu button opens the drawer; the button inside the drawer closes it |

The Tauri window is at least 940×620, which is `compact`. At 200% zoom it is 470px wide, which is
`narrow`. All three layouts therefore occur in real use.

Script holds only two facts the stylesheet cannot work out, and writes both on `.shell`:

- `data-collapsed="true"`: the wide sidebar is folded to its rail. This is the remembered
  preference, read before the first paint, and it has an effect only in `wide`.
- `data-drawer="open"`: the drawer is open. It has an effect only in `compact` and `narrow`.
  Crossing a breakpoint closes it.

`useLayoutMode()` (`shell/useLayout.ts`) returns the current mode. It is read synchronously on the
first render and updated on the media queries' `change` events, not on every resize. The shell uses
it only to decide what the sidebar button says and does, through `sidebarToggle(mode, collapsed,
drawerOpen)`, and never to change geometry. `useSidebarCollapsed()` reads and writes the preference
inside `try`/`catch`; when storage is unavailable the full sidebar is the default, because it hides
nothing.

### What the sidebar shows

The sidebar's contents follow its own width, not the window's. `.sidebar` is a size container named
`sidebar`, so no rule needs to know which layout made it narrow:

- `@container sidebar (max-width: 120px)` is the rail. Labels are clipped from view but stay in the
  DOM, so a screen reader still reads "Approvals, 2 waiting on you". The approvals count becomes a
  badge on the icon, the palette launcher becomes a 40px icon button, and the theme control becomes
  one button that cycles. Tooltips open to the right of the rail.
- `@container sidebar (min-width: 121px)` is the full sidebar. The tooltips that stand in for labels
  are switched off. The two buttons that keep a tooltip, the sidebar toggle and the shortcuts button,
  open it inwards from their trailing edge, because the full sidebar scrolls and would clip anything
  drawn outside it.

### The content column

Each screen is drawn in `.content`: `max-width: var(--content-max)`, centred, with side padding of
`var(--gutter)`, a little less at the top and twice as much at the foot. A screen gets these
responsive rules without writing any:

- `.grid.stats` is `repeat(auto-fit, minmax(min(180px, 100%), 1fr))`, and `.grid.two` the same at
  320px.
- `.row` wraps its trailing chips under the title rather than drawing over it, and a long word in
  `.row-meta` breaks anywhere.
- Under 720px, `dl.facts` and `dl.args` inside `.content` stack each label over its value.
- `.toolbar` wraps, with a text filter taking the slack.

### Shell anatomy

`Shell.tsx` only composes. Each part is its own module in `src/shell/`.

| Module | Responsibility |
| --- | --- |
| `Shell.tsx` | Composition. Holds the overlay, drawer and collapse state. Starts the event stream and installs the spotlight listener once. Keeps scroll memory per address, names the window after the screen, and moves focus to `main` after a move. Makes `main` inert while the drawer is open. |
| `layout.ts` | Pure decisions: `layoutMode(width)`, `mediaQuery(mode)`, `sidebarPresentation(mode, collapsed, drawerOpen)` (`full`, `rail`, `drawer` or `hidden`), `sidebarToggle(…)`, `applySidebarAction(…)`, the preference codec, `SIDEBAR_KEY` and `SIDEBAR_ID`. Tested in `layout.test.ts`. |
| `useLayout.ts` | `useLayoutMode()` and `useSidebarCollapsed()`. |
| `Sidebar.tsx` | The brand and the sidebar button, `PaletteLauncher`, `NavList`, and the foot (`ConnectionStatus`, `ThemeToggle`, the keyboard shortcuts button). While it is a drawer it is `role="dialog" aria-modal="true" aria-label="Navigation"`. |
| `drawer.ts` | `useDrawer(ref, open, onClose)`: the modal contract for an element that stays mounted and is only sometimes a modal. |
| `modal.ts` | `useModal` for the palette and dialogs, and the shared `focusables()`, `trapTab()` and `modalOpen()`. |
| `NavList.tsx` | The screen buttons (`aria-current="page"`) and one sliding `.nav-indicator`, measured before paint and re-measured by a `ResizeObserver`. It slides only from the frame after the first paint. |
| `TopBar.tsx` | The narrow layout only. The menu button (`aria-expanded`, `aria-controls="sidebar"`), the brand, the current screen's name, a fixture-data mark, the palette button, and the approvals pill with its count. `inert` while the drawer is open. |
| `PaletteLauncher.tsx` | A button shaped like a search field, "Search or jump to…", with a hidden `⌘K` or `Ctrl K` key cap and `aria-keyshortcuts`. |
| `ThemeToggle.tsx`, `themeChoices.ts` | Match the system, dark and light, as three `aria-pressed` segments with a sliding thumb, or one cycling button in the rail. |
| `ConnectionStatus.tsx` | "Fixture data — not connected to a runtime" in the attention hue (`role="status"`), or a pulsing `LiveDot` with "Local runtime". When the event feed's last read failed it reads "Runtime not answering" and the dot turns to attention and stops pulsing. |
| `ApprovalsBadge.tsx` | `ApprovalsCount` pops when the count changes and marks itself stale when the read failed, keeping the last count. `ApprovalsAnnouncer` is the polite live region, mounted outside the sidebar so it is still heard in the narrow layout. |
| `Brand.tsx` | The shield mark on a tile in the accent gradient. |
| `Screen.tsx` | Maps a route to its screen. |
| `nav.ts` | `NAV`: each screen's route, label, description and icon, in sidebar order. One list serves the sidebar, the `Cmd/Ctrl+1…7` accelerators, the palette and the window title. |
| `keys.ts`, `useShellKeys.ts` | The window accelerators as a pure function of one key press, and the listener that carries them out. They stand aside while any `[aria-modal="true"]` is open, the drawer included. |
| `palette.ts` | The palette's entries, ranking and icons, as pure functions. |
| `ShellAlerts.tsx`, `notices.ts` | Alerts for failed runs as they stream in, and for a store that cannot reach the runtime. |
| `CloseGuardHost.tsx`, `closeMessage.ts` | Asks before the window closes on live work. The message is a pure function. |

The order inside `.shell` is:

1. The skip link.
2. `header.topbar`.
3. `#sidebar.sidebar`.
4. `.sidebar-scrim`.
5. `main#main.main` > `.content` > the screen's error boundary > `.screen.screen-enter`, keyed on
   `route.name`.
6. The approvals announcer, the alert hosts, the palette or shortcut sheet when open, the leave
   confirmation and the close guard.

The screen wrapper is keyed on the route's name, not its address, so a new screen rises in and a
change of parameter, such as opening a run in Tasks, does not remount it.

## Motion vocabulary

### Keyframes

The keyframes in `motion.css` are a contract: screens use these names and do not declare their own.

| Keyframe | Movement | Used by |
| --- | --- | --- |
| `screen-enter` | Fade in and rise 8px | `.screen-enter`, `.stagger > *`, the top bar's screen name |
| `pop` | Scale 1 → 1.18 → 1 | `.pop` (the approvals count) |
| `pulse-ring` | Scale 1 → 2.8 while fading | `.live-dot.pulsing::after`, live and running verdict chips |
| `sheen` | `translateX(-100%)` → `translateX(100%)` | `.sheen::after`, a hovered `button.primary` |
| `shimmer` | Background position sweep | `.skeleton`, `.skeleton-line` |
| `overlay-in` | Fade in and scale 0.96 → 1 | `.dialog`, `.palette` |
| `backdrop-in` | Fade in | `.dialog-backdrop`, `.palette-backdrop`, `.chart-tip` |
| `toast-in` | From beyond the right edge, fading in | `.alert`, with `--ease-spring` |
| `grow-x` | `scaleX(0)` → none; the element sets its own `transform-origin` | `.chart-span`, `.chart-bar`, `.sparkline` |
| `countdown` | `scaleX(1)` → `scaleX(0)` | `.alert-countdown` |
| `drawer-in` | From `translateX(-100%)` | Nothing at present. The drawer slides by a transform transition, so that it can slide out as well |

### Utilities

```tsx
<div key={route.name} className="screen-enter">…</div>   {/* the shell does this; key on the name */}
<ul className="plain-list stagger">{items}</ul>          {/* children 35ms apart, capped at the 12th */}
<span key={key} className={popping ? "nav-count pop" : "nav-count"}>{count}</span>
<div className="progress-fill sheen" />                  {/* a band of light crosses while in progress */}
```

- `.screen-enter` and `.stagger > *` run `screen-enter` over `--dur-3` on `--ease-out`. From the
  twelfth child on, a staggered list shares the last delay, so a long list is never kept waiting for
  its tail. Use them on block or inline-block elements: an inline element cannot be transformed and
  would only fade.
- `.pop` runs over `--dur-4` on `--ease-spring` and sets `display: inline-block`. Because
  `motion.css` loads before the layout and component sheets, a class there that sets `display` at
  equal specificity still wins.
- `.sheen` takes over the element's `::after` and sets `position: relative; overflow: hidden`. The
  band crosses every `2 × --dur-shimmer` for as long as the class is present, so put it only on
  something that is still in progress.
- `.stagger` and `.sheen` are ready to use but no screen uses them yet.

### Entrances fill backwards, never forwards

A `forwards` fill would leave a `transform` on the element after it arrived. A transform makes an
element the containing block of every `position: fixed` descendant, so a dialog rendered inside an
entered screen would be pinned to the screen instead of the window. Filling `backwards` holds the
first frame through a stagger's delay and leaves nothing behind. The one `forwards` animation is the
alert countdown, which is not an entrance and holds no fixed content.

### Overlays, alerts and charts

- `.dialog-backdrop` and `.palette-backdrop` fade in over `--dur-2`. `.dialog` and `.palette` run
  `overlay-in` over `--dur-3` on `--ease-spring`.
- `.alert` runs `toast-in` over `--dur-4` on `--ease-spring`.
- Chart spans and bars run `grow-x` over `--dur-4` on `--ease-out`. It plays once on insertion, so
  re-rendering the same spans does not replay it.
- The chart tip fades in over `--dur-1`. It must not use `overlay-in`, because its own `transform`
  positions it.

### Transitions

- A button's colours move at `--dur-2`; its press moves at `--dur-1`, so a click feels immediate.
- The narrow drawer slides on a `transform` transition over `--dur-3`. Its `visibility` waits for the
  slide out to finish before hiding and returns at once on the way in, so the drawer is never seen
  without being reachable.
- The shell's grid columns ease over `--dur-3` when the wide sidebar folds.
- The nav marker slides on `--ease-spring` over `--dur-3`; the theme thumb does the same.
- A pinned page header's glass and title scale fade over `--dur-3`.

### Motion in script

`components/motion.ts` covers what a stylesheet cannot animate. Every helper honours reduced motion
itself. The arithmetic (`cubicBezier`, `easeOut`, `tweenValue`, `decimalsOf`, `roundLike`, `nextPop`,
`spotlightPoint`) is pure and tested in `motion.test.ts`; the hooks only schedule it.

| Export | What it does |
| --- | --- |
| `prefersReducedMotion()` | Whether the person has asked for less motion. Check it before scheduling anything in script. |
| `useCountUp(target, duration = COUNT_UP_MS)` | A number that counts to `target` (600ms by default) on the same curve as `--ease-out`. It starts from zero on first draw and from the value on screen after that, so an interrupted count never jumps backwards. Each frame is rounded to the target's own precision, so a count to 12 passes through 7 and 9, never 8.4137. Under reduced motion it lands at once, and a timer settles it if frames stop because the window is hidden. |
| `usePop(value)` | Returns `{ key, popping }`. Key the badge on `key` and add `.pop` while `popping`. It pops on every change and never on the first draw. |
| `installSpotlight(document)` | The one delegated `pointermove` listener behind `[data-spotlight]`. The shell installs it; screens never do. |

### Reduced motion

The duration tokens already go to near zero. On top of that, the loops and entrances are removed
outright, because a loop of near-zero length is a flicker and an entrance would hold content
invisible for a frame: `.screen-enter`, `.stagger > *`, `.pop`, `.sheen::after`, skeletons, overlays,
alerts, chart spans, bars and the tip, and the sparkline all have `animation: none`. The live ring is
hidden and its dot stays. The alert countdown stays as a full bar: time still passes, shown without
movement. `base.css` keeps a global override as a safety net for any rule that names a duration of
its own, and turns off smooth scrolling.

## Primitives

### Importing

Screens import everything from `components/common`, which re-exports the primitives beside the older
shared pieces:

```tsx
import {
  CopyButton, Empty, EmptyState, ErrorBanner, Icon, Kbd, LiveDot, Loading, PageHeader, Panel,
  Risk, Row, SkeletonRows, Sparkline, Stale, StatTile, Switch, Tainted, Verdict, tip,
} from "../../components/common";
```

Styles for the primitives are in `styles/primitives.css`. The motion hooks are imported from
`components/motion` directly.

### Icon

`components/icons.tsx`. Original line drawings on a 24-unit grid: a 1.75 stroke, round caps and
joins, in `currentColor`, so an icon takes the colour of the text beside it. `ICON_NAMES` lists every
name, `IconName` is their union, and `isIconName(string)` checks a name that arrives as data.

- Screens: `dashboard approvals tasks schedules agents activity settings`.
- Chrome and controls: `search command menu close chevron-left chevron-right chevron-down
  sidebar-collapse sidebar-expand sun moon monitor check x alert info play pause stop retry plus copy
  external filter trash edit lock key globe terminal folder window cpu zap eye shield sparkles clock
  calendar user link refresh more`.

| Prop | Type | Default | |
| --- | --- | --- | --- |
| `name` | `IconName` | required | |
| `size` | `number` | `18` | Rendered pixels |
| `label` | `string` | none | Without it the icon is `aria-hidden`. With it the icon is `role="img"` with that name |
| `className` | `string` | none | Added after `icon` |

```tsx
<Icon name="shield" />                       // decorative, beside text that says the same
<Icon name="alert" label="Tainted run" />    // stands alone and carries meaning
```

An icon-only control is named on the control, not the icon:
`<button type="button" className="icon-button" {...tip("Retry")}><Icon name="retry" /></button>`.
`icons.test.ts` checks that every name has a drawing and that every screen in `NAV` has a distinct
icon.

### Icon button (`.icon-button`)

A class, not a component. 32×32, or 40×40 on a coarse pointer. Transparent at rest, a raised fill
and a subtle edge on hover, and it scales to 0.92 when pressed. Always pair it with `tip()` or an
`aria-label`.

```tsx
<button type="button" className="icon-button" {...tip("Refresh")}>
  <Icon name="refresh" />
</button>
```

### Tooltip (`data-tip`, `tip()`)

`components/Tooltip.ts`. Tooltips are drawn by the stylesheet from `data-tip` with `::after`. On
hover a tooltip appears after 380ms, so a pointer crossing a row of icons does not set off a flicker
of labels; on keyboard focus it appears at once. On a screen with no hover a tap would leave it stuck
on, so it is suppressed there.

`tip(label, side = "bottom")` returns `{ "aria-label", "data-tip", "data-tip-side"? }` for the
usual case, where the name and the tooltip are the same words. `side` is `"bottom"`, `"top"` (for a
control near the foot of the window) or `"right"` (for the rail).

```tsx
<button type="button" className="icon-button" {...tip("Copy the run id")}>
  <Icon name="copy" />
</button>
```

A tooltip is never the only name. Assistive technology does not read `data-tip`, so the element must
also have an `aria-label` or keep its own text in the DOM, as nav items do in the rail. A tooltip is
clipped by any scrolling or `overflow: hidden` box it sits in, and `.panel` is one: use tooltips on
chrome and toolbars, not inside cards.

### PageHeader

`components/PageHeader.tsx`. The title block of a screen, top to bottom: an optional breadcrumb, an
eyebrow, the title row (an `h1` with the actions beside it), and a subtitle.

Only the title row pins. Once the main column scrolls past it, it becomes a floating glass bar 8px
below the top and its title scales to 0.8. A zero-height sentinel and an `IntersectionObserver`
detect the moment, and the component adds `.stuck`; nothing runs on scroll. The row's box grows out
by exactly its own padding, so the title does not move when it pins. In the narrow layout the row
does not pin, because the top bar already names the screen.

| Prop | Type | Default |
| --- | --- | --- |
| `title` | `ReactNode` | required |
| `eyebrow` | `ReactNode` | none |
| `subtitle` | `ReactNode` | none |
| `actions` | `ReactNode` | none |
| `parent` | `{ label: string; onActivate: () => void }` | none. A back crumb with a chevron |
| `sticky` | `boolean` | `true` |

```tsx
<PageHeader
  eyebrow="Workforce"
  title="Agents"
  subtitle="Each agent has its own instructions, its own workspace, and its own policy."
  actions={<button type="button" className="primary">New agent</button>}
/>

<PageHeader title={run.id} parent={{ label: "Tasks", onActivate: () => navigate({ name: "tasks" }) }} />
```

`PageHeader` returns a fragment. Render it as a direct child of the screen's root element, so the
pinned row can stick for the whole length of the screen. Tasks, Run, Schedules, Agents and Settings
use it. Dashboard, Approvals and Activity still write `.page-head` by hand: they get the 26px title
but do not pin until they move to `PageHeader`.

### StatTile

`components/StatTile.tsx`. A figure on a raised card: an icon and a label, a value that counts up, an
optional delta or hint, and an optional sparkline along its foot. With `onClick` it is a real
`<button>` with `.lift`, `data-spotlight` and a chevron. Without it, it is a quiet card.

| Prop | Type | Default | |
| --- | --- | --- | --- |
| `label` | `string` | required | Shown as an eyebrow |
| `value` | `number \| string \| null` | required | A number counts up. A string, such as "verified", is shown as written. `null` shows "—" while loading, and the first number then counts up from zero |
| `format` | `(n: number) => string` | `toLocaleString()` | Formats the number, including the frames in between |
| `icon` | `IconName` | none | |
| `hint` | `ReactNode` | none | |
| `delta` | `{ text: string; direction: "up" \| "down" \| "flat"; sentiment?: "good" \| "bad" \| "neutral" }` | none | `direction` sets the arrow; `sentiment` sets the colour |
| `trend` | `readonly number[]` | none | A sparkline, oldest first. A `critical` tile draws it in the danger hue |
| `tone` | `"accent" \| "ok" \| "warn" \| "danger" \| "critical" \| "info"` | `"accent"` | Colours the icon tile, and the figure for every tone but `accent`. `warn`, `danger` and `critical` also give the card an edge and glow in the hue |
| `onClick` | `() => void` | none | Makes the tile a button |

```tsx
<div className="grid stats">
  <StatTile label="Agents" value={agents.data?.length ?? null} icon="agents" trend={runsPerDay} />
  <StatTile
    label="Waiting on you"
    value={pending}
    icon="approvals"
    tone={pending > 0 ? "warn" : "accent"}
    onClick={() => navigate({ name: "approvals" })}
  />
  <StatTile label="Audit chain" value="verified" icon="shield" tone="ok" hint="checked 2m ago" />
</div>
```

The moving digits are `aria-hidden`; a visually hidden span gives assistive technology the final
figure, so a number is never read part-way. No screen uses `StatTile` yet: the dashboard still draws
the older `Stat`.

### EmptyState

`components/EmptyState.tsx`. A whole panel or screen with nothing in it: what is missing, why that is
fine or what to do, and the one action that fills it. It is `role="status"`, because an empty result
is an answer the person asked for. For a one-line empty list inside a panel, `Empty` is still right.

| Prop | Type | Default |
| --- | --- | --- |
| `title` | `ReactNode` | required |
| `icon` | `IconName` | `"sparkles"` |
| `children` | `ReactNode` | none. One or two sentences under the title |
| `action` | `ReactNode` | none. One or two buttons |

```tsx
<Panel>
  <EmptyState
    icon="agents"
    title="No agents yet"
    action={<button type="button" className="primary" onClick={create}>Create an agent</button>}
  >
    An agent is a model with tools and a policy.
  </EmptyState>
</Panel>
```

### Switch

`components/Switch.tsx`. An on/off control that takes effect at once: a setting, not a form field
waiting for Save. It is a real `<button role="switch" aria-checked>`, and the stylesheet draws the
state from `aria-checked`, so what is drawn and what is announced cannot disagree. A 36×20 track, a
knob that springs across and stretches towards where it is going while pressed, and the accent
gradient when on. The hit target is at least 32px tall, 40px on a coarse pointer.

| Prop | Type | |
| --- | --- | --- |
| `checked` | `boolean` | |
| `onChange` | `(checked: boolean) => void` | |
| `label` | `ReactNode` | A visible label, which is also the name |
| `ariaLabel` | `string` | The name when the words that name it sit elsewhere on the row |
| `disabled` | `boolean` | |

```tsx
<Switch checked={running} onChange={setRunning} label="Scheduler running" />
```

### CopyButton

`components/CopyButton.tsx`. Copies `text` through `useCopy` (`components/hooks.ts`), which strips
terminal escape sequences and control characters first, so text copied from tool output cannot
rewrite a terminal it is pasted into. The copy icon turns into a check for 1.5s, or into the alert
mark if the webview refused. A polite live region that is always present says "Copied" or "Could not
copy".

| Prop | Type | Default | |
| --- | --- | --- | --- |
| `text` | `string` | required | |
| `label` | `string` | `"Copy"` | The name and tooltip, such as "Copy the run id" |
| `showLabel` | `boolean` | `false` | Draws the words as well. Otherwise it is an icon button |

```tsx
<CopyButton text={run.id} label="Copy the run id" />
```

`useCopy(feedbackMs = 2000)` takes how long its result stands before the control resets.

### Kbd

`components/Kbd.tsx`. A key, or a short chord written as one. Renders `<kbd class="kbd">`, which
`base.css` draws as a small raised key cap. Pass `hidden` when the shortcut is already announced
another way, through `aria-keyshortcuts` or the shortcut sheet, so it is not read twice.

```tsx
<Kbd hidden>{apple ? "⌘K" : "Ctrl K"}</Kbd>
```

### LiveDot

`components/LiveDot.tsx`. A dot whose ring pulses (`pulse-ring`, `--dur-pulse`) for as long as the
thing it marks is live, and only then. Under reduced motion the ring stops and the dot stays.

| Prop | Type | Default |
| --- | --- | --- |
| `tone` | `"ok" \| "accent" \| "warn" \| "danger" \| "info"` | `"ok"` |
| `pulse` | `boolean` | `true` |
| `label` | `string` | none. Without it the dot is decorative and the text beside it must carry the meaning |

```tsx
<span className="inline"><LiveDot tone="info" />Executing</span>
```

### Sparkline

`components/Sparkline.tsx`. A small SVG line with a soft fill beneath it. It fills its parent's box
and keeps its stroke width when stretched. Counts are drawn from zero, not from their own minimum, so
a series that moved from 100 to 104 looks nearly flat. Its accessible name is the label plus the
series in words, for example "Runs per day: 7 values, from 2 to 12; lowest 2, highest 12". It grows
in from the left. The geometry and the words are pure functions in `sparklinePath.ts`, tested in
`sparklinePath.test.ts`.

| Prop | Type | Default |
| --- | --- | --- |
| `values` | `readonly number[]` | required |
| `label` | `string` | required |
| `tone` | `"accent" \| "ok" \| "warn" \| "danger" \| "info"` | `"accent"` |

`accent` and `info` both draw in `--info-fg`; `ok`, `warn` and `danger` draw in their hue's `fg`.

```tsx
<div className="runs-spark"><Sparkline values={series} label="Runs per day" /></div>
```

Give it a sized box through a class in the screen's stylesheet; a screen does not set sizes inline.
`StatTile` sizes its own.

### Status chips and marks

`components/status.tsx`. Two shapes, because they answer two questions. A `.risk` chip is a property
of an action, drawn as an outline in small capitals; `high` glows faintly and `critical` fills and
glows. A `.verdict` chip is what happened, drawn filled as a pill in sentence case. Each chip opens
with a visually hidden noun ("risk", "outcome", "run", "task", "agent"), so it is read in context.

| Component | Prop | Shows |
| --- | --- | --- |
| `Risk` | `level` | `none`, `low`, `medium`, `high` or `critical` |
| `Verdict` | `outcome` | A tool call's outcome |
| `State` | `state` | A run state; a live run carries a pulsing dot |
| `Status` | `status` | A task status |
| `Enabled` | `status` | Whether an agent may be given work, deliberately not a run state |
| `Tainted` | `label?`, `explanation?` | The mark for anything that has read untrusted data |

```tsx
<Risk level={call.risk} /> <Verdict outcome={call.outcome} />
{run.tainted ? <Tainted /> : null}
```

`Tainted` gives its explanation as visually hidden text rather than a `title`, which a keyboard never
reaches. Show it wherever a tainted run appears, not only on the approval card.

### Lists, loading and failure

From `components/common.tsx`:

| Component | Use |
| --- | --- |
| `Panel` | A raised card (`.panel`) |
| `Row` | One line of a list. With `onActivate` it is a real `<button>` carrying `data-spotlight`; without it, a plain `<div>`. Pass `onActivate` only when activation does something, and never to a row that holds its own buttons |
| `Empty` | A one-line empty list, `role="status"` |
| `Loading` | "Loading … ", `role="status"`, polite |
| `SkeletonRows` | Placeholder rows shaped like a row's title and meta, hidden from assistive technology. Pair it with `Loading`, which is what speaks |
| `ErrorBanner` | An async failure, `role="alert"` |
| `Stale` | "showing data from 3m ago", for a screen whose last refresh failed |
| `Stat` | The older labelled number. Prefer `StatTile` in new work |

```tsx
<Panel>
  {agents.loading ? (<><Loading what="agents" /><SkeletonRows count={2} /></>) : null}
  {agents.data?.length === 0 ? <Empty>No agents yet.</Empty> : null}
  <div role="list" aria-label="Agents">
    {agents.data?.map((agent) => (
      <div role="listitem" key={agent.id}>
        <Row onActivate={() => navigate({ name: "agents", agent: agent.name })}>…</Row>
      </div>
    ))}
  </div>
</Panel>
```

### Shared classes

The look of bare elements and the classes every screen uses:

- **Buttons** (`base.css`). At rest, a `--surface-raised` fill, the `--border-control` edge and
  `--highlight-inset`; on hover, `--surface-hover` and `--border-control-hover`; pressed, scale 0.97
  at `--dur-1`. Rows, palette entries and nav items are exempt from the press. `button.primary` is the
  accent gradient, laid under its border too; on hover it darkens, takes `--accent-glow` and sends one
  sheen across beneath the label (it isolates its stacking, so it gives up its `::after`).
  `button.danger` is danger-edged with `--danger-text` and glows on hover. `button.ghost` is
  transparent and dim until hovered. Any button with `aria-pressed="true"` takes the accent wash.
- **Fields** are sunken wells (`--surface-sunken`, `--shadow-well`) with the control edge. The edge
  brightens on hover, and focus adds an accent border beside the ring. Placeholders are
  `--text-faint`. Checkboxes, radios and ranges keep their native shape in the accent colour.
- **`.segmented`** is a two-to-four way choice: a sunken track with the chosen segment as a raised
  key (`aria-pressed`). The key's fill and shadow cross-fade at hover speed as the choice moves.
- **`.check`** is a selectable card for a checkbox; `.check.on` takes the accent wash, and its note
  moves from faint to dim text.
- **`.banner`** (`.error`, `.info`, `.warn`) is a rounded sentence the page wants read first, with a
  3px bar in the hue on its leading edge.
- **`.approval`** is the approval card. While undecided it has the warn edge and glow, the same hue
  as the approvals count; `.selected` takes the accent line, `.focused` the accent ring and glow,
  `.decided` goes quiet. Its footer is sticky glass, so Approve and Deny stay on screen beside large
  arguments.
- **`.tag`**, **`pre.code`** (`.wrap` for arguments awaiting a decision), **`dl.facts`** and
  **`dl.args`** are the fact and code shapes.
- **`.stale`**, **`.muted`**, **`.faint`**, **`.mono`**, **`.prose`**, **`.field-note`**,
  **`.visually-hidden`**, **`.inline`**, **`.stack`**, **`.spacer`** are the utilities.
- **`kbd`** is a small raised key cap. Scrollbars are thin in `--border-strong`, and the selection is
  `--accent-line`.

## Interaction patterns

### Spotlight

Add `data-spotlight` to a card or row that does something when pressed. The shell's one delegated
`pointermove` listener writes `--mx` and `--my` on the hovered element, at most once a frame and
never for touch. Its `::before` draws a 280px radial light in `--spotlight` at that point, fading in
over `--dur-3`. `[data-spotlight]` gets `position: relative` and `isolation: isolate` at zero
specificity, and the light sits at `z-index: -1` inside it: above its fill, below its content. An
element with `data-spotlight` gives up its `::before`. On a screen with no hover the light is off.

### Lift

Add `.lift` to anything pressable that stands on its own, such as a card or a tile. On hover it rises
1px, its edge becomes `--border-strong` and its shadow `--elev-2`; pressed, it scales to 0.985 at
`--dur-1`. A pressable card takes both: `<button type="button" className="panel lift"
data-spotlight>`. A `Row` with `onActivate` is already lit, and rows do not lift: a row that rises
out of its list breaks the list's edges.

### Pop and count-up

A count that changed pops once (`usePop` and `.pop`), and a figure counts to its new value
(`useCountUp`). Neither plays on first draw except a tile's first number, which counts up from zero
so it arrives rather than appears. Both are described under [Motion in script](#motion-in-script).

### The sliding indicator

The current nav item sits on one shared marker that slides between items on `--ease-spring`, so a
move between screens is seen as a move. Its leading bar is drawn in `--accent-gradient` with
`--accent-glow`. It does not slide on the first paint, since there is nowhere it came from. The
theme control's thumb slides the same way.

### The modal contract

The palette, the shortcut sheet and every dialog keep `useModal` (`shell/modal.ts`): focus moves in
when it opens, Tab cycles inside it, Escape closes it, and focus returns to wherever it was when it
closes, if that element is still in the document. A press on the backdrop closes it too. While any
`[aria-modal="true"]` is open, the window's accelerators stand aside.

### The drawer

While open, the drawer is a modal dialog (`shell/drawer.ts`):

- Focus starts on the current screen's item, so Enter takes the person nowhere new.
- Tab is trapped, and Escape, a press on the scrim, or a move to another screen closes it.
- `main` and the top bar are `inert`, so neither a pointer nor the keyboard can reach the page.
- On close, focus returns to the opener only if it was still in the drawer or had fallen to `body`.
  A move to another screen has already put focus on that screen, and taking it back would undo that.
- The palette launcher and shortcuts button inside the drawer close it before opening their overlay.

### The command palette

`Cmd/Ctrl+K` or the launcher opens it, and the same key closes it. It lists screens, in sidebar
order, and agents, read afresh each time it opens. Ranking (`shell/palette.ts`) prefers the label to
the description, and a closer match to a looser one: whole label, then its start, then a run of it,
then its letters in order. Each entry leads with its screen's icon, which becomes a gradient tile on
the active entry, and the active entry shows `↵` and a 2px accent bar on its leading edge. Arrow keys
move, Enter goes, Escape closes. An entry is a route, never a callback, so the palette cannot
approve, stop a run or disable an agent.

### Alerts

Alerts stack in the top-right corner, below the top bar under 720px, and below any overlay. Each
leads with an icon for its level (`info`, `alert`, `x`) and a 3px edge in its hue, and has a dismiss
button named "Dismiss: …". Warnings and errors are `role="alert"`; notes are `role="status"`. A
non-sticky alert leaves after `ALERT_TTL_MS` (6s) and shows a countdown bar along its foot, which
restarts if the same alert is raised again. Errors are always sticky. Past four alerts, the oldest
non-sticky one makes way; when every alert on screen is sticky, a new one is shown anyway, because a
condition that was never shown is worse than a crowded corner. Only live events raise run failures:
history read on start is the record, not news.

### Confirmation

`ConfirmDialog` asks a yes-or-no question inside the window, because Tauri's webview is not
guaranteed to show `window.confirm`. It is an `alertdialog` with a warn mark beside the title. Focus
starts on the answer that changes nothing, which is the primary button, and Escape and the backdrop
give the same answer; the destructive answer is `danger`. The close guard and the unsaved-changes
guard both use it.

### Keyboard

| Keys | Does |
| --- | --- |
| `Cmd/Ctrl+1` … `7` | Go to a screen, in sidebar order |
| `Cmd/Ctrl+,` | Go to Settings, even while typing |
| `Cmd/Ctrl+K` | Open or close the palette |
| `Cmd/Ctrl+[` or `Alt+←` | Back |
| `Cmd/Ctrl+]` or `Alt+→` | Forward |
| `?` | The shortcut sheet |
| `Esc` | Close the palette, the sheet, a question or the drawer |

No bare letter is bound globally, because a screen binds its own (the approvals queue moves with `j`
and `k`). Everything but `Cmd/Ctrl+,` is ignored while the person is typing. The sheet is written from
the same facts `commandFor` reads, so the two cannot drift.

## Accessibility rules

These are requirements, not suggestions. Most are already enforced by a shared piece; a screen keeps
them by using that piece rather than writing its own.

1. **Contrast.** Text clears 4.5:1 and control edges and the focus ring clear 3:1 on every ground
   they can land on, in both themes, as recomputed by `stylesheets.test.ts`. Use the text tokens on
   the grounds the tables cover, and keep faint text off hovered controls and washes.
2. **Focus is always visible.** `focus.css` draws one ring for everything, only when focus arrived by
   keyboard: a 2px `--focus` outline at a 2px offset, and a 6px `--focus-glow` halo. While a control
   is focused the halo replaces its own shadow, so the ring wins. Rows, palette entries, segmented
   buttons and the palette input draw it inside. `main` takes focus on navigation but draws nothing.
   SVG chart spans use a `--focus` stroke. A component may move the ring; it may never remove it.
3. **Colour is never the only signal.** Pair every hue with a shape, an icon or words.
4. **Every control has a name.** Icon-only controls use `tip()` or `aria-label`; a tooltip is never
   the only name. A decorative icon is `aria-hidden`; an icon that carries meaning alone has a
   `label`.
5. **Real elements for real behaviour.** A clickable row is a `<button>` (`Row` with `onActivate`), a
   switch is `role="switch"`, a toggle uses `aria-pressed`, the current screen is
   `aria-current="page"`. Interactive content never nests inside a button.
6. **Overlays keep the modal contract.** Focus in, Tab trapped, Escape out, focus restored. Use
   `useModal`; the drawer uses `useDrawer`.
7. **Changes are announced.** Live regions are present before their first message, or the first
   message is not spoken: `ApprovalsAnnouncer` and `CopyButton` show the pattern. Empty results and
   loading are `role="status"`, failures `role="alert"`. Anything visual that duplicates them, such as
   skeleton rows or a key cap, is hidden from assistive technology.
8. **Numbers are read whole.** Animated figures are `aria-hidden` beside a visually hidden final
   value. A sparkline's name includes the series in words. A chart is a figure with a caption and a
   table carrying the same facts as text.
9. **Reduced motion is honoured** in CSS through the tokens and in script through
   `prefersReducedMotion()`. Nothing is held invisible, nothing loops.
10. **Targets suit the pointer.** Icon buttons and switches are 32px; under `(pointer: coarse)` every
    button is at least 40px tall, icon buttons 40×40, and nav items and the palette launcher 44px.
11. **The page reflows at 200% zoom.** At that zoom the minimum window is in the narrow layout. Rows
    wrap, fact lists stack, and the shell's columns give way rather than holding their widest size;
    `stylesheets.test.ts` checks the rules this depends on.
12. **Navigation is predictable.** The skip link is the first Tab stop. After a move, focus goes to
    `main`, so the next Tab starts at the page, and the window is named after the screen.
13. **Untrusted text is shown, not obeyed.** Text the person may copy is sanitised by `useCopy`;
    arguments awaiting a decision wrap so nothing hides past the right edge; a tainted run is marked
    wherever it appears.

## How to build a screen

A checklist for whoever owns a screen. The route item applies only to a new screen.

**Structure**

- [ ] The screen lives in `src/routes/<screen>/`, entered through `index.ts`, with everything it
      draws in that folder.
- [ ] Its decisions — filtering, grouping, wording, what a state means — are pure functions in
      `<screen>Model.ts`, tested in `<screen>Model.test.ts`. The component only draws.
- [ ] A new screen adds its variant to `Route` in `routes/route.ts`, its address to `shell/hash.ts`
      with a round-trip test, its entry to `NAV` in `shell/nav.ts` (with a distinct icon), and its
      case to `shell/Screen.tsx`. The accelerator, palette entry and window title follow from `NAV`.
- [ ] It receives its route's parameters and `navigate` as props and never reaches into the shell.

**Page frame**

- [ ] `PageHeader` is the first child of the screen's root fragment, with a `title`, a `subtitle` of
      one sentence, and the screen's primary action as a `button.primary` in `actions`. A detail
      screen passes `parent` for the way back.
- [ ] Sections are introduced with `h2`. There is one `h1`, and `PageHeader` draws it.

**Content**

- [ ] Figures go in `.grid.stats` as `StatTile`s. A tile that leads somewhere gets `onClick`. A tone
      other than `accent` is used only when the figure asks for attention.
- [ ] Lists are `Panel`s of `Row`s inside `role="list"` and `role="listitem"`, or a `.plain-list`.
      A row gets `onActivate` only when activation does something.
- [ ] Cards that do something use `.lift` with `data-spotlight`. Static content uses neither.
- [ ] `.stagger` is used only on a list whose arrival order means something.
- [ ] Risk, outcome, run and task states use the status chips, never a hand-coloured span. Anything
      that read untrusted data shows `Tainted`.
- [ ] Live things are marked with `LiveDot` or a live verdict, and stop moving when they stop.

**States**

- [ ] Loading shows `Loading` beside `SkeletonRows`; data comes through `useAsyncCached` (or
      `useAsync`), so a revisit shows cached data at once.
- [ ] A refresh that fails keeps the data on screen and shows `Stale`; a load that fails with nothing
      to show uses `ErrorBanner`.
- [ ] A whole empty panel uses `EmptyState` with the action that fills it; a one-line empty list uses
      `Empty`.
- [ ] A form with unsaved input keeps it as a draft and guards leaving (`sdk/drafts.ts`).
- [ ] A destructive action asks through `ConfirmDialog`, with the safe answer focused.

**Style**

- [ ] New classes go in `styles/screens/<screen>.css` and read tokens only: no colour literal, no
      duration or easing that is not a token, no spacing off the `--s-*` scale.
- [ ] Nothing is styled inline. Geometry that only the data knows is passed as a custom property that
      a class reads, as in `style={{ "--w": "62%" }}`.
- [ ] A shape another screen could use belongs in `layout.css`, `components.css` or
      `primitives.css`, not in the screen's sheet.

**Checks**

- [ ] The screen works in all three layouts: at 1440px, at 1000px, and at 200% zoom of the minimum
      window (under 720px).
- [ ] It works in both themes, with nothing hard to read on a hovered row or a wash.
- [ ] Every control is reachable and named from the keyboard, the focus ring is visible on each, and
      focus is never lost behind an overlay.
- [ ] With reduced motion on, nothing moves and nothing is held invisible.
- [ ] `npm test` and `npm run check` pass, including `npx vitest run src/styles` if a token changed.
