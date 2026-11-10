import type { ReactNode } from "react";

/**
 * The application's one icon set: original line drawings on a 24-unit grid.
 *
 * Every drawing is strokes in `currentColor`, so an icon takes the colour of
 * the text beside it and the light theme reaches it with no rule of its own. A
 * few small marks — the dot of an `i`, a pupil — are filled instead, because a
 * stroked dot that small reads as a smudge. No icon package: the set is small,
 * the drawings are part of the product's look, and a dependency of a thousand
 * glyphs to use fifty is weight the window would carry for nothing.
 */

/** Every icon, by name. The list is the registry's contract; see icons.test.ts. */
export const ICON_NAMES = [
  // The screens.
  "dashboard",
  "approvals",
  "tasks",
  "schedules",
  "agents",
  "activity",
  "settings",
  // The chrome and the controls.
  "search",
  "command",
  "menu",
  "close",
  "chevron-left",
  "chevron-right",
  "chevron-down",
  "sidebar-collapse",
  "sidebar-expand",
  "sun",
  "moon",
  "monitor",
  "check",
  "x",
  "alert",
  "info",
  "play",
  "pause",
  "stop",
  "retry",
  "plus",
  "copy",
  "external",
  "filter",
  "trash",
  "edit",
  "lock",
  "key",
  "globe",
  "terminal",
  "folder",
  "database",
  "window",
  "cpu",
  "zap",
  "eye",
  "shield",
  "sparkles",
  "clock",
  "calendar",
  "user",
  "link",
  "refresh",
  "more",
] as const;

export type IconName = (typeof ICON_NAMES)[number];

/** A filled dot: the marks too small to draw as a ring. */
function Dot({ x, y, r = 1.1 }: { x: number; y: number; r?: number }) {
  return <circle cx={x} cy={y} r={r} fill="currentColor" stroke="none" />;
}

/**
 * The drawings. Typed as a complete record, so a name added to the list
 * without a drawing does not compile.
 */
const DRAWINGS: Record<IconName, ReactNode> = {
  dashboard: (
    <>
      <rect x="3.5" y="3.5" width="7" height="9" rx="1.75" />
      <rect x="13.5" y="3.5" width="7" height="5" rx="1.75" />
      <rect x="13.5" y="11.5" width="7" height="9" rx="1.75" />
      <rect x="3.5" y="15.5" width="7" height="5" rx="1.75" />
    </>
  ),
  approvals: (
    <>
      <rect x="4.5" y="4.5" width="15" height="16.5" rx="2.25" />
      <path d="M9 4.5v-.75c0-.69.56-1.25 1.25-1.25h3.5c.69 0 1.25.56 1.25 1.25v.75" />
      <path d="M8.75 13l2.25 2.25 4.25-4.75" />
    </>
  ),
  tasks: (
    <>
      <path d="M10.5 6.5H20M10.5 12H20M10.5 17.5H20" />
      <path d="M4 6.5l1.5 1.5L8 5.25" />
      <path d="M4 12l1.5 1.5L8 10.75" />
      <circle cx="6" cy="17.5" r="1.6" />
    </>
  ),
  schedules: (
    <>
      <path d="M20 12a8 8 0 1 1-2.34-5.66" />
      <path d="M20 4.25v4h-4" />
      <path d="M12 8v4.25l2.75 1.75" />
    </>
  ),
  agents: (
    <>
      <rect x="4.5" y="7.5" width="15" height="11.5" rx="3.25" />
      <path d="M12 7.5V4.75" />
      <Dot x={12} y={3.75} r={1.25} />
      <Dot x={9.25} y={12.75} r={1.35} />
      <Dot x={14.75} y={12.75} r={1.35} />
      <path d="M10 16h4" />
      <path d="M2.5 12v2.5M21.5 12v2.5" />
    </>
  ),
  activity: <path d="M3 12h4l2.5-6.5 5 13 2.5-6.5h4" />,
  settings: (
    <>
      <path d="M4 7h8.5M17.5 7H20M4 17h2.5M11.5 17H20" />
      <circle cx="15" cy="7" r="2.5" />
      <circle cx="9" cy="17" r="2.5" />
    </>
  ),

  search: (
    <>
      <circle cx="10.75" cy="10.75" r="6.25" />
      <path d="M20 20l-4.75-4.75" />
    </>
  ),
  // The Command key's four loops, drawn as a square whose sides run on into them.
  command: (
    <>
      <path d="M9 6.5v11M15 6.5v11M6.5 9h11M6.5 15h11" />
      <path d="M9 6.5a2.5 2.5 0 1 0-2.5 2.5" />
      <path d="M15 6.5a2.5 2.5 0 1 1 2.5 2.5" />
      <path d="M9 17.5a2.5 2.5 0 1 1-2.5-2.5" />
      <path d="M15 17.5a2.5 2.5 0 1 0 2.5-2.5" />
    </>
  ),
  menu: <path d="M4 7h16M4 12h16M4 17h16" />,
  close: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  "chevron-left": <path d="M14.5 6l-6 6 6 6" />,
  "chevron-right": <path d="M9.5 6l6 6-6 6" />,
  "chevron-down": <path d="M6 9.5l6 6 6-6" />,
  "sidebar-collapse": (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2.75" />
      <path d="M9.5 4.5v15" />
      <path d="M15.75 9.5L13.25 12l2.5 2.5" />
    </>
  ),
  "sidebar-expand": (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2.75" />
      <path d="M9.5 4.5v15" />
      <path d="M13.25 9.5l2.5 2.5-2.5 2.5" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="3.75" />
      <path d="M12 2.75v1.75M12 19.5v1.75M2.75 12H4.5M19.5 12h1.75M5.46 5.46l1.24 1.24M17.3 17.3l1.24 1.24M5.46 18.54l1.24-1.24M17.3 6.7l1.24-1.24" />
    </>
  ),
  moon: <path d="M19.5 14.6A7.75 7.75 0 0 1 9.4 4.5a7.75 7.75 0 1 0 10.1 10.1z" />,
  monitor: (
    <>
      <rect x="3" y="4.5" width="18" height="12" rx="2.25" />
      <path d="M8.5 20h7M12 16.5V20" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  x: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M9.25 9.25l5.5 5.5M14.75 9.25l-5.5 5.5" />
    </>
  ),
  alert: (
    <>
      <path d="M10.27 4.3L3.1 17a2 2 0 0 0 1.73 3h14.34a2 2 0 0 0 1.73-3L13.73 4.3a2 2 0 0 0-3.46 0z" />
      <path d="M12 9.5v4" />
      <Dot x={12} y={16.75} />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.25" />
      <Dot x={12} y={7.9} />
    </>
  ),
  play: <path d="M8 5.75v12.5a.75.75 0 0 0 1.14.64l9.9-6.25a.75.75 0 0 0 0-1.28l-9.9-6.25A.75.75 0 0 0 8 5.75z" />,
  pause: <path d="M9 5.5v13M15 5.5v13" />,
  stop: <rect x="6" y="6" width="12" height="12" rx="2.25" />,
  retry: (
    <>
      <path d="M4 12a8 8 0 1 0 2.34-5.66" />
      <path d="M4 4.25v4h4" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  copy: (
    <>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2.25" />
      <path d="M15.5 8.5V6.25c0-.97-.78-1.75-1.75-1.75h-7.5c-.97 0-1.75.78-1.75 1.75v7.5c0 .97.78 1.75 1.75 1.75H8.5" />
    </>
  ),
  external: (
    <>
      <path d="M14 4.5h5.5V10" />
      <path d="M19.5 4.5L11 13" />
      <path d="M18 14v4.25c0 .69-.56 1.25-1.25 1.25H6.25C5.56 19.5 5 18.94 5 18.25V7.75c0-.69.56-1.25 1.25-1.25H10" />
    </>
  ),
  filter: <path d="M4 5.5h16l-6.25 7.75v5.25L10.25 20v-6.75z" />,
  trash: (
    <>
      <path d="M4.5 7h15" />
      <path d="M9.5 7V5.25c0-.69.56-1.25 1.25-1.25h2.5c.69 0 1.25.56 1.25 1.25V7" />
      <path d="M6.5 7l.85 11.6c.06.79.72 1.4 1.5 1.4h6.3c.78 0 1.44-.61 1.5-1.4L17.5 7" />
      <path d="M10 11v5M14 11v5" />
    </>
  ),
  edit: (
    <>
      <path d="M15.25 4.75l4 4L8.75 19.25 4 20l.75-4.75z" />
      <path d="M13 7l4 4" />
    </>
  ),
  lock: (
    <>
      <rect x="5" y="10.5" width="14" height="10" rx="2.25" />
      <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" />
      <path d="M12 14.5v2" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15.5" r="4" />
      <path d="M10.85 12.65L19.5 4" />
      <path d="M16.25 7.25l2.5 2.5M13.75 9.75l2 2" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17" />
      <path d="M12 3.5c2.4 2.3 3.75 5.2 3.75 8.5S14.4 18.2 12 20.5c-2.4-2.3-3.75-5.2-3.75-8.5S9.6 5.8 12 3.5z" />
    </>
  ),
  terminal: (
    <>
      <rect x="3" y="4.5" width="18" height="15" rx="2.75" />
      <path d="M7 9.5l3 2.5-3 2.5" />
      <path d="M12.5 15H17" />
    </>
  ),
  folder: (
    <path d="M3.5 7.25c0-1.1.9-2 2-2h3.9l2.1 2.5h7c1.1 0 2 .9 2 2v7.5c0 1.1-.9 2-2 2h-13c-1.1 0-2-.9-2-2z" />
  ),
  // A stack of three discs seen from the side: the top as an ellipse, the
  // seams beneath it as the same curve's front half.
  database: (
    <>
      <ellipse cx="12" cy="6" rx="7.25" ry="2.75" />
      <path d="M4.75 6v12c0 1.52 3.25 2.75 7.25 2.75s7.25-1.23 7.25-2.75V6" />
      <path d="M4.75 12c0 1.52 3.25 2.75 7.25 2.75s7.25-1.23 7.25-2.75" />
    </>
  ),
  window: (
    <>
      <rect x="3" y="4.5" width="18" height="15" rx="2.75" />
      <path d="M3 9h18" />
      <Dot x={6.25} y={6.75} r={0.7} />
      <Dot x={8.75} y={6.75} r={0.7} />
    </>
  ),
  cpu: (
    <>
      <rect x="6.5" y="6.5" width="11" height="11" rx="2.25" />
      <rect x="9.75" y="9.75" width="4.5" height="4.5" rx="1" />
      <path d="M9.5 3.5v3M14.5 3.5v3M9.5 17.5v3M14.5 17.5v3M3.5 9.5h3M3.5 14.5h3M17.5 9.5h3M17.5 14.5h3" />
    </>
  ),
  zap: <path d="M13.25 2.75L4.75 13.5h6.5l-1 7.75 8.5-11h-6.5z" />,
  eye: (
    <>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="2.75" />
    </>
  ),
  shield: <path d="M12 3l7.5 3v5.5c0 4.5-3.15 8.2-7.5 9.5-4.35-1.3-7.5-5-7.5-9.5V6z" />,
  sparkles: (
    <>
      <path d="M10 3.5l1.65 4.35L16 9.5l-4.35 1.65L10 15.5l-1.65-4.35L4 9.5l4.35-1.65z" />
      <path d="M17.75 14l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  calendar: (
    <>
      <rect x="3.5" y="5" width="17" height="15.5" rx="2.75" />
      <path d="M3.5 10h17M8 3v4M16 3v4" />
    </>
  ),
  user: (
    <>
      <circle cx="12" cy="8.5" r="3.75" />
      <path d="M4.75 20c.8-3.6 3.65-5.75 7.25-5.75S18.45 16.4 19.25 20" />
    </>
  ),
  link: (
    <>
      <path d="M10 14a4 4 0 0 0 5.66 0l3-3A4 4 0 0 0 13 5.34l-1 1" />
      <path d="M14 10a4 4 0 0 0-5.66 0l-3 3A4 4 0 0 0 11 18.66l1-1" />
    </>
  ),
  refresh: (
    <>
      <path d="M4.5 11.25a7.5 7.5 0 0 1 13.2-4.1L19.5 9" />
      <path d="M19.5 4.5V9H15" />
      <path d="M19.5 12.75a7.5 7.5 0 0 1-13.2 4.1L4.5 15" />
      <path d="M4.5 19.5V15H9" />
    </>
  ),
  more: (
    <>
      <Dot x={5.5} y={12} r={1.4} />
      <Dot x={12} y={12} r={1.4} />
      <Dot x={18.5} y={12} r={1.4} />
    </>
  ),
};

/** Whether `name` is in the registry. For data that names an icon at run time. */
export function isIconName(name: string): name is IconName {
  return Object.hasOwn(DRAWINGS, name);
}

/**
 * One icon.
 *
 * Decorative by default — hidden from assistive technology, because the text
 * or label beside it already says what it means. Give it a `label` only where
 * the icon stands alone and carries meaning, and it becomes an image with that
 * name. A control holding only an icon is named on the control, with
 * `aria-label`, not here.
 */
export function Icon({
  name,
  size = 18,
  label,
  className,
}: {
  name: IconName;
  /** Rendered size in pixels; the drawing scales from its 24-unit grid. */
  size?: number | undefined;
  label?: string | undefined;
  className?: string | undefined;
}) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      {...(label === undefined
        ? { "aria-hidden": true }
        : { role: "img", "aria-label": label })}
    >
      {DRAWINGS[name]}
    </svg>
  );
}
