/**
 * Time, as the schedules screen says it.
 *
 * Every function here is pure over an explicit `now`, so a test fails when one
 * of them changes. Nothing reads the clock, and nothing formats through
 * `toLocaleString`, whose output differs between webviews.
 */

export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

/** Two digits, as a clock writes them. */
export function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** Milliseconds since the epoch, or `NaN` for a time that cannot be read. */
export function timeOf(iso: string | null | undefined): number {
  return iso ? Date.parse(iso) : Number.NaN;
}

/**
 * A length of time as its largest whole unit: `4m`, `22h`, `6d`.
 *
 * Rounded down, so a firing twenty-two and a half hours away is never said to
 * be twenty-three hours away and then arrive early.
 */
export function lengthOf(ms: number): string {
  if (ms < HOUR) return `${Math.max(1, Math.floor(ms / MINUTE))}m`;
  if (ms < DAY) return `${Math.floor(ms / HOUR)}h`;
  return `${Math.floor(ms / DAY)}d`;
}

/**
 * A length of time as its two largest whole units, for a countdown: `22h 28m`,
 * `6d 3h`, `4m`.
 *
 * Rounded down for the same reason as {@link lengthOf}. A second unit of zero
 * is dropped rather than written, so six days exactly reads `6d`, not `6d 0h`.
 * Under a minute there is nothing honest to count, so it says so.
 */
export function countdown(ms: number): string {
  if (!Number.isFinite(ms) || ms < MINUTE) return "<1m";
  const days = Math.floor(ms / DAY);
  const hours = Math.floor((ms % DAY) / HOUR);
  const minutes = Math.floor((ms % HOUR) / MINUTE);
  if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  return `${minutes}m`;
}

/**
 * How far `iso` lies from `now`, in either direction: `in 22h`, `30m ago`.
 *
 * `ago` in the shared formatter clamps the future to "just now", which is
 * right for things that happened and wrong for every `next_run_at` on this
 * screen. Within a minute either side reads `now`. A time that cannot be read
 * is returned as it came, rather than as a confident nonsense length.
 */
export function relative(iso: string, now: number): string {
  const at = timeOf(iso);
  if (!Number.isFinite(at)) return iso;
  const delta = at - now;
  if (Math.abs(delta) < MINUTE) return "now";
  return delta > 0 ? `in ${lengthOf(delta)}` : `${lengthOf(-delta)} ago`;
}

/**
 * A moment as a short local date and time: `Tue 7 Oct 09:00`.
 *
 * Built from the date's own fields rather than `toLocaleString`, so it reads
 * the same in every webview and a column of firings lines up.
 */
export function localMoment(ms: number): string {
  const at = new Date(ms);
  const weekday = WEEKDAYS[at.getDay()] ?? "";
  const month = MONTHS[at.getMonth()] ?? "";
  return `${weekday} ${at.getDate()} ${month} ${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

/** A month's short name from its number, 1 to 12; `null` outside that. */
export function monthName(month: number): string | null {
  return MONTHS[month - 1] ?? null;
}

/**
 * One previewed firing, in local time.
 *
 * A cron expression read in UTC also gets its UTC wall time beside it, because
 * that is the clock the operator typed the expression in: `0 9 * * *` should be
 * seen to land on 09:00 UTC, not only on whatever that is here.
 */
export function firingLabel(iso: string, clock: string | null): string {
  const at = timeOf(iso);
  if (!Number.isFinite(at)) return iso;
  const local = localMoment(at);
  if (clock !== "utc") return local;
  const utc = new Date(at);
  return `${local} (${pad(utc.getUTCHours())}:${pad(utc.getUTCMinutes())} UTC)`;
}

/**
 * A moment as an `<input type="datetime-local">` holds it: `2026-10-06T14:30`,
 * in the window's own time zone, to the minute.
 */
export function toLocalInput(ms: number): string {
  const at = new Date(ms);
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `T${pad(at.getHours())}:${pad(at.getMinutes())}`
  );
}

const LOCAL_INPUT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/;

/**
 * The moment a `datetime-local` value names, or `NaN`.
 *
 * Parsed by hand rather than through `Date.parse`, whose reading of a string
 * without an offset has differed between engines. A date that does not exist,
 * such as the thirty-first of September, is refused rather than rolled over
 * into October: a one-shot that fires a day late because of a typo is the
 * silent kind of wrong this screen exists to prevent.
 */
export function fromLocalInput(text: string): number {
  const match = LOCAL_INPUT.exec(text.trim());
  if (!match) return Number.NaN;
  const [year, month, day, hour, minute, second] = match
    .slice(1)
    .map((part) => (part === undefined ? 0 : Number(part)));
  const at = new Date(year!, month! - 1, day!, hour!, minute!, second!);
  const exists =
    at.getFullYear() === year &&
    at.getMonth() === month! - 1 &&
    at.getDate() === day &&
    at.getHours() === hour &&
    at.getMinutes() === minute;
  return exists ? at.getTime() : Number.NaN;
}

/**
 * Seconds as the units a person counts in: `6h`, `1h 30m`, `90s` → `1m 30s`.
 *
 * The runtime describes an interval as `every 21600s`, exactly and
 * unhelpfully; this is said beside it, never instead of it.
 */
export function spanOfSeconds(seconds: number): string {
  if (!Number.isSafeInteger(seconds) || seconds <= 0) return `${seconds}s`;
  const parts: string[] = [];
  let rest = seconds;
  for (const [unit, size] of [
    ["d", 86_400],
    ["h", 3_600],
    ["m", 60],
    ["s", 1],
  ] as const) {
    if (rest >= size) {
      parts.push(`${Math.floor(rest / size)}${unit}`);
      rest %= size;
    }
  }
  return parts.join(" ");
}
