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
 * rail. The element is given `position: relative` by the rule that draws it,
 * unless a class of its own positions it.
 */

/** Which side of its element a tooltip opens on. */
export type TipSide = "bottom" | "top" | "right";

/** The attributes that give an element a tooltip and the same accessible name. */
export function tip(
  label: string,
  side: TipSide = "bottom",
): { "aria-label": string; "data-tip": string; "data-tip-side"?: TipSide } {
  return side === "bottom"
    ? { "aria-label": label, "data-tip": label }
    : { "aria-label": label, "data-tip": label, "data-tip-side": side };
}
