/** Presentation helpers. Shared so every screen speaks about time and risk the same way. */

/** A short relative time, e.g. `4m ago`. */
export function ago(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;

  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (seconds < 10) return "just now";
  if (seconds < 60) return `${seconds}s ago`;

  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  return `${Math.round(hours / 24)}d ago`;
}

/** Wall-clock time, for the activity feed where ordering matters more than recency. */
export function clock(iso: string): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return iso;
  return at.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** A duration in milliseconds, rendered compactly. */
export function duration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms / 60_000)}m`;
}

/** Token counts, which get long. */
export function tokens(count: number): string {
  if (count < 1000) return `${count}`;
  return `${(count / 1000).toFixed(1)}k`;
}

/** Whether a run state means the run is still going. */
export function isLive(state: string): boolean {
  return !["completed", "failed", "cancelled", "idle"].includes(state);
}

/** Turn a snake_case wire value into something readable. */
export function humanise(value: string): string {
  return value.replace(/_/g, " ");
}
