import type { ReactNode } from "react";

import { Icon, type IconName } from "./icons";
import { useCountUp } from "./motion";
import { type SparkTone, Sparkline } from "./Sparkline";

/** A tile's hue. `accent` is the neutral default; the rest are status tones. */
export type StatTone = "accent" | "ok" | "warn" | "danger" | "critical" | "info";

/** A change since the last period, drawn as an arrow and a few words. */
export interface StatDelta {
  /** The words, such as "+3 today" or "−12% vs last week". */
  text: string;
  direction: "up" | "down" | "flat";
  /** Whether this direction is good news here; it sets the colour, not the arrow. */
  sentiment?: "good" | "bad" | "neutral" | undefined;
}

/** What a tile shows while its figure is still being read. */
const PENDING = "—";

/**
 * A figure on a raised card: an icon and a label, the number counting up to
 * its value, an optional hint or change beneath, and an optional sparkline.
 *
 * The tone is a class, never a colour chosen here, so the light theme reaches
 * it; a status tone also gives the card a faint glow in its hue, for the
 * figures that ask for attention. With `onClick` the tile is a real button —
 * focus, Enter, Space and the announcement come from the platform — that
 * lifts and lights under the pointer; without it, it is a quiet card.
 *
 * A number counts from the last value shown (from zero when first drawn), on
 * the same ease-out as the stylesheets, and lands at once under reduced
 * motion. The moving digits are hidden from assistive technology, which is
 * given the final figure instead, so a screen reader never reads a number
 * part-way.
 */
export function StatTile({
  label,
  value,
  format = defaultFormat,
  icon,
  hint,
  delta,
  trend,
  tone = "accent",
  onClick,
}: {
  label: string;
  /** A number counts up; a string, such as "verified", is shown as written; `null` is still loading. */
  value: number | string | null;
  /** How a number is written; whole numbers are grouped by locale by default. */
  format?: ((value: number) => string) | undefined;
  icon?: IconName | undefined;
  hint?: ReactNode;
  delta?: StatDelta | undefined;
  /** A series for the sparkline under the figure, oldest first. */
  trend?: readonly number[] | undefined;
  tone?: StatTone | undefined;
  onClick?: (() => void) | undefined;
}) {
  // Counted towards zero while there is no number, so the first real figure
  // counts up from nothing rather than appearing.
  const shown = useCountUp(typeof value === "number" ? value : 0);
  const display = typeof value === "number" ? format(shown) : (value ?? PENDING);
  const final = typeof value === "number" ? format(value) : (value ?? "not yet known");

  const body = (
    <>
      <span className="stat-tile-head">
        {icon ? (
          <span className="stat-tile-icon" aria-hidden="true">
            <Icon name={icon} size={16} />
          </span>
        ) : null}
        <span className="stat-tile-label">{label}</span>
        {onClick ? <Icon name="chevron-right" size={14} className="stat-tile-go" /> : null}
      </span>
      <span className="stat-tile-value">
        <span aria-hidden="true">{display}</span>
        <span className="visually-hidden">{final}</span>
      </span>
      {delta || hint ? (
        <span className="stat-tile-foot">
          {delta ? <Delta delta={delta} /> : null}
          {hint ? <span className="stat-tile-hint">{hint}</span> : null}
        </span>
      ) : null}
      {trend && trend.length > 0 ? (
        <span className="stat-tile-spark">
          <Sparkline values={trend} label={`${label}, trend`} tone={sparkTone(tone)} />
        </span>
      ) : null}
    </>
  );

  const className = `stat-tile tone-${tone}`;
  if (onClick) {
    return (
      <button type="button" className={`${className} lift`} data-spotlight onClick={onClick}>
        {body}
      </button>
    );
  }
  return (
    <div className={className}>
      {body}
    </div>
  );
}

function Delta({ delta }: { delta: StatDelta }) {
  const arrow = delta.direction === "up" ? "↑" : delta.direction === "down" ? "↓" : "→";
  return (
    <span className={`stat-delta ${delta.sentiment ?? "neutral"}`}>
      <span aria-hidden="true">{arrow} </span>
      {delta.text}
    </span>
  );
}

function sparkTone(tone: StatTone): SparkTone {
  return tone === "critical" ? "danger" : tone;
}

function defaultFormat(value: number): string {
  return value.toLocaleString();
}
