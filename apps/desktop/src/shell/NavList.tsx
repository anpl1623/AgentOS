import {
  type CSSProperties,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { Icon } from "../components/icons";
import type { Route, RouteName } from "../routes/route";
import { ApprovalsCount } from "./ApprovalsBadge";
import { NAV, screenRoute } from "./nav";

/**
 * The screens, as a list of buttons with one sliding marker behind the
 * current one.
 *
 * The marker is a single element that moves to whichever item is current,
 * rather than a background on each item, so a move between screens is seen
 * as a move. It does not slide on the first paint — there is nowhere it came
 * from — only once the window is showing.
 *
 * Each item keeps its label in the DOM in every layout. The rail hides it from
 * the eye (the sidebar's container query) and shows it again as a tooltip from
 * `data-tip`; assistive technology reads the label, and the approvals count
 * with it, whatever the width.
 */
export function NavList({
  current,
  onNavigate,
}: {
  current: RouteName;
  onNavigate: (route: Route) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const { box, ready } = useMarker(track, current);

  return (
    <div className="nav-track" ref={track}>
      {box === null ? null : (
        <span
          className="nav-indicator"
          aria-hidden="true"
          data-ready={ready ? "" : undefined}
          style={{ "--y": `${box.top}px`, "--h": `${box.height}px` } as CSSProperties}
        />
      )}
      <ul className="nav-list">
        {NAV.map((item) => {
          const active = item.route === current;
          return (
            <li key={item.route}>
              <button
                type="button"
                className="nav-item"
                aria-current={active ? "page" : undefined}
                data-tip={item.label}
                data-tip-side="right"
                onClick={() => onNavigate(screenRoute(item.route))}
              >
                <Icon name={item.icon} className="nav-icon" />
                <span className="nav-label">{item.label}</span>
                {item.route === "approvals" ? <ApprovalsCount /> : null}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

interface Box {
  top: number;
  height: number;
}

/**
 * Where the marker sits: the current item's box within the track, measured
 * before paint and again whenever the track is resized — the sidebar
 * collapsing, the drawer opening, text being enlarged.
 */
function useMarker(
  track: RefObject<HTMLDivElement | null>,
  current: RouteName,
): { box: Box | null; ready: boolean } {
  const [box, setBox] = useState<Box | null>(null);
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const element = track.current;
    if (element === null) return;
    const measure = () => {
      const item = element.querySelector<HTMLElement>('[aria-current="page"]');
      const next = item === null ? null : { top: item.offsetTop, height: item.offsetHeight };
      setBox((previous) =>
        previous !== null && next !== null && previous.top === next.top && previous.height === next.height
          ? previous
          : next,
      );
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [track, current]);

  // Allowed to slide only from the frame after the first paint.
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => setReady(true));
    return () => window.cancelAnimationFrame(frame);
  }, []);

  return { box, ready };
}
