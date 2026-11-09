import { useId } from "react";

import { SPARK_BOX, describeSeries, sparklineGeometry } from "./sparklinePath";

/** The hues a sparkline can be drawn in. */
export type SparkTone = "accent" | "ok" | "warn" | "danger" | "info";

/**
 * A small line of a series, with a soft fill beneath it.
 *
 * It fills the box its parent gives it — the stroke keeps its width however
 * the box stretches — and is named for assistive technology with the series
 * in words, so the trend is told as well as drawn.
 */
export function Sparkline({
  values,
  label,
  tone = "accent",
}: {
  values: readonly number[];
  /** What the series is, such as "Runs per day"; the numbers are added to it. */
  label: string;
  tone?: SparkTone | undefined;
}) {
  // useId's characters are not all safe inside url(#…); the letters are enough
  // to keep it unique.
  const fill = `spark${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const { line, area } = sparklineGeometry(values);
  return (
    <svg
      className={`sparkline tone-${tone}`}
      viewBox={`0 0 ${SPARK_BOX.width} ${SPARK_BOX.height}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`${label}: ${describeSeries(values)}`}
    >
      <defs>
        <linearGradient id={fill} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" className="sparkline-stop-top" />
          <stop offset="1" className="sparkline-stop-bottom" />
        </linearGradient>
      </defs>
      {area === "" ? null : <path className="sparkline-area" d={area} fill={`url(#${fill})`} />}
      {line === "" ? null : (
        <path className="sparkline-line" d={line} vectorEffect="non-scaling-stroke" />
      )}
    </svg>
  );
}
