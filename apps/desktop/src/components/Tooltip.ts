/**
 * Tooltips, drawn by the stylesheet from a `data-tip` attribute.
 *
 * A tooltip is never the only name a control has. Assistive technology does
 * not read `data-tip`, and a pointer that never hovers never sees it, so the
 * element must also be named — by `aria-label`, or by its own text kept in
 * the DOM. `tip()` sets both from one string, which is the usual case: an
 * icon-only button whose tooltip and name are the same words.
 *
 *   <button type="button" className="icon-button" {...tip("Copy the run id")}>
 *     <Icon name="copy" />
 *   </button>
 *
 * The tooltip appears below the element on hover after a short delay, and at
 * once on keyboard focus; `side: "top"` places it above, for a control near
 * the foot of the window, and `side: "right"` to the right, for the sidebar's
 * rail. Above or below, it is centred on its element unless `align: "end"`
 * lines it up with the element's trailing edge, for a control at the end of a
 * row: centred there, it would reach past the column and give it a sideways
 * scroll. The element is given `position: relative` by the rule that draws it,
 * unless a class of its own positions it.
 */

/** Which side of its element a tooltip opens on. */
export type TipSide = "bottom" | "top" | "right";

/** Where a tooltip above or below its element lines up with it. */
export type TipAlign = "center" | "end";

/** The attributes that give an element a tooltip and the same accessible name. */
export function tip(
  label: string,
  side: TipSide = "bottom",
  align: TipAlign = "center",
): {
  "aria-label": string;
  "data-tip": string;
  "data-tip-side"?: TipSide;
  "data-tip-align"?: TipAlign;
} {
  return {
    "aria-label": label,
    "data-tip": label,
    ...(side === "bottom" ? {} : { "data-tip-side": side }),
    ...(align === "center" ? {} : { "data-tip-align": align }),
  };
}
