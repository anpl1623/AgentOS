import { SkeletonRows } from "../components/common";

/**
 * What stands in for a screen while its code is fetched: the shape of a
 * screen's frame — eyebrow, title, a sentence, a panel of rows — in the
 * skeleton shimmer the screens use for their own data.
 *
 * Fetching is usually quicker than a person can see, so the stylesheet holds
 * this invisible for a moment before it fades in, and a quick fetch never
 * flashes it at all (`.screen-pending`). The status line is for assistive
 * technology, which is told the screen is on its way only if it is slow.
 */
export function ScreenFallback() {
  return (
    <div className="screen-pending" aria-busy="true">
      <p className="visually-hidden" role="status">
        Opening the screen…
      </p>
      <div className="screen-pending-head" aria-hidden="true">
        <span className="skeleton screen-pending-eyebrow" />
        <span className="skeleton screen-pending-title" />
        <span className="skeleton screen-pending-sub" />
      </div>
      <div className="panel">
        <SkeletonRows count={4} />
      </div>
    </div>
  );
}
