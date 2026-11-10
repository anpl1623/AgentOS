import { type ReactNode, useId, useState } from "react";

import { Icon, type IconName } from "./icons";
import { useCountUp } from "./motion";
import { type SparkTone, Sparkline } from "./Sparkline";

/**
 * A tile's hue. `accent` is the neutral default; the rest are status tones,
 * and `blocked` is the hue a refusal is drawn in everywhere else.
 */
export type StatTone = "accent" | "ok" | "warn" | "danger" | "critical" | "info" | "blocked";

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
 * it; the loud tones also give the card a faint glow in their hue, for the
 * figures that ask for attention.
 *
 * The card is the same element whatever it does, so it is never remounted:
 * a tile whose action comes and goes keeps its place in a staggered row
 * without playing its entrance again. With `onClick` the label becomes a real
 * button — focus, Enter, Space and the announcement come from the platform —
 * whose hit area is stretched over the whole card, and the card lifts and
 * lights under the pointer. A tile that has once been a button stays one for
 * as long as it is drawn, unavailable (`aria-disabled`) while there is nothing
 * to do: a control that vanished from under the keyboard would drop focus to
 * the top of the window. A tile that has never had an action is a quiet card,
 * not a button that can never be pressed.
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
  trendLabel,
  trendCaption,
  tone = "accent",
  className,
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
  /**
   * What the series is, for the sparkline's accessible name, such as "Calls
   * per day, the last 7 days"; the numbers are added to it. The label and
   * "trend" by default.
   */
  trendLabel?: string | undefined;
  /**
   * A few words drawn over the sparkline saying what it counts and how far
   * back, such as "per day, last 7 days", since a line carries no axis.
   * Hidden from assistive technology, which hears `trendLabel` instead.
   */
  trendCaption?: string | undefined;
  tone?: StatTone | undefined;
  /** Added after the tile's own classes, for a screen's layout. */
  className?: string | undefined;
  onClick?: (() => void) | undefined;
}) {
  // Counted towards zero while there is no number, so the first real figure
  // counts up from nothing rather than appearing.
  const shown = useCountUp(typeof value === "number" ? value : 0);
  const display = typeof value === "number" ? format(shown) : (value ?? PENDING);
  const final = typeof value === "number" ? format(value) : (value ?? "not yet known");

  // Latched: set during render, as React allows for state derived from props,
  // so the button appears in the same commit as the action that asks for it.
  const [control, setControl] = useState(onClick !== undefined);
  if (onClick !== undefined && !control) setControl(true);
  const active = onClick !== undefined;

  const id = useId();
  const labelId = `${id}label`;
  const valueId = `${id}value`;
  const footId = `${id}foot`;
  const foot = Boolean(delta || hint);

  const classes = ["stat-tile", `tone-${tone}`, active ? "is-action lift" : null, className]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} data-spotlight={active ? "" : undefined}>
      <span className="stat-tile-head">
        {icon ? (
          <span className="stat-tile-icon" aria-hidden="true">
            <Icon name={icon} size={16} />
          </span>
        ) : null}
        {control ? (
          // Named by the label and the final figure together, and described
          // by the line beneath, so focus arriving on it hears the whole tile.
          <button
            type="button"
            id={labelId}
            className="stat-tile-label stat-tile-hit"
            aria-labelledby={`${labelId} ${valueId}`}
            aria-describedby={foot ? footId : undefined}
            aria-disabled={active ? undefined : true}
            onClick={onClick}
          >
            {label}
          </button>
        ) : (
          <span id={labelId} className="stat-tile-label">
            {label}
          </span>
        )}
        {active ? <Icon name="chevron-right" size={14} className="stat-tile-go" /> : null}
      </span>
      <span className="stat-tile-value">
        <span aria-hidden="true">{display}</span>
        {/* Hidden once the button's name carries it, so reading on past the
            button does not hear the figure twice. */}
        <span id={valueId} className="visually-hidden" aria-hidden={control || undefined}>
          {final}
        </span>
      </span>
      {foot ? (
        <span id={footId} className="stat-tile-foot">
          {delta ? <Delta delta={delta} /> : null}
          {hint ? <span className="stat-tile-hint">{hint}</span> : null}
        </span>
      ) : null}
      {trend && trend.length > 0 && trendCaption ? (
        <span className="stat-tile-trend-caption" aria-hidden="true">
          {trendCaption}
        </span>
      ) : null}
      {trend && trend.length > 0 ? (
        <span className="stat-tile-spark">
          <Sparkline
            values={trend}
            label={trendLabel ?? `${label}, trend`}
            tone={sparkTone(tone)}
          />
        </span>
      ) : null}
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

/**
 * The sparkline's hue for a tile's tone. A sparkline has no critical or
 * blocked hue of its own: critical draws in danger, and blocked in the accent
 * family a refusal belongs to.
 */
function sparkTone(tone: StatTone): SparkTone {
  if (tone === "critical") return "danger";
  if (tone === "blocked") return "accent";
  return tone;
}

function defaultFormat(value: number): string {
  return value.toLocaleString();
}
