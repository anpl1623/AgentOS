/**
 * Cadences in words a person reads at a glance: `Weekdays at 09:00`,
 * `Every 6 hours`, `Monthly on the 1st at 09:00`.
 *
 * Words are said beside the runtime's own description, never instead of it,
 * and only when they are certainly right. The cron grammar here is the one the
 * runtime parses with (the `cron` crate): an optional leading seconds field, an
 * optional trailing year, and days of the week counted from Sunday as 1, so
 * `1-5` is Sunday to Thursday. An expression this module does not fully
 * understand gets no words at all, and the screen shows the expression itself;
 * a confident wrong sentence about when an agent acts is worse than none.
 */

import type { IconName } from "../../components/icons";
import type { CadenceForm, CadenceVerdict } from "./formModel";
import { DEFAULT_EXPRESSION, cadenceInput } from "./formModel";
import { monthName, pad } from "./timeModel";

/**
 * The mark for a kind of cadence, the same on a schedule's card and on the
 * form's choice of kind, so the two are recognisably the same thing: a loop
 * for an interval, a calendar for cron, a clock for one moment.
 */
export function cadenceIcon(kind: string): IconName {
  switch (kind) {
    case "every":
      return "refresh";
    case "cron":
      return "calendar";
    case "once":
      return "clock";
    default:
      return "schedules";
  }
}

// ---------------------------------------------------------------------------
// Intervals
// ---------------------------------------------------------------------------

const UNITS = [
  ["week", 604_800],
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
  ["second", 1],
] as const;

/**
 * An interval in the largest unit that divides it exactly: `Every 6 hours`,
 * `Every 90 minutes`, `Every week`. Exact, so `5400` is ninety minutes rather
 * than "about an hour and a half". `null` for a value that is not a positive
 * whole number of seconds.
 */
export function intervalWords(seconds: number): string | null {
  if (!Number.isSafeInteger(seconds) || seconds <= 0) return null;
  for (const [unit, size] of UNITS) {
    if (seconds % size === 0) {
      const count = seconds / size;
      return count === 1 ? `Every ${unit}` : `Every ${count} ${unit}s`;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Cron
// ---------------------------------------------------------------------------

/** The days as the runtime numbers them: index 1 is Sunday, 7 is Saturday. */
const DAY_SHORT = ["", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const DAY_PLURAL = [
  "",
  "Sundays",
  "Mondays",
  "Tuesdays",
  "Wednesdays",
  "Thursdays",
  "Fridays",
  "Saturdays",
] as const;

/** Every spelling of a day the runtime accepts, case aside. */
const DAY_NAMES: Readonly<Record<string, number>> = {
  sun: 1,
  sunday: 1,
  mon: 2,
  monday: 2,
  tue: 3,
  tues: 3,
  tuesday: 3,
  wed: 4,
  wednesday: 4,
  thu: 5,
  thurs: 5,
  thursday: 5,
  fri: 6,
  friday: 6,
  sat: 7,
  saturday: 7,
};

const MONTH_NAMES = [
  "jan",
  "feb",
  "mar",
  "apr",
  "may",
  "jun",
  "jul",
  "aug",
  "sep",
  "oct",
  "nov",
  "dec",
] as const;
const MONTH_FULL = [
  "january",
  "february",
  "march",
  "april",
  "may",
  "june",
  "july",
  "august",
  "september",
  "october",
  "november",
  "december",
] as const;

function any(field: string): boolean {
  return field === "*" || field === "?";
}

function numberIn(field: string, min: number, max: number): number | null {
  if (!/^\d+$/.test(field)) return null;
  const value = Number(field);
  return value >= min && value <= max ? value : null;
}

/**
 * `*\/n` or `0/n`: every `n` from the start of a range of `size` values.
 *
 * Only a step that divides the range evenly is accepted. `*\/7` in the minute
 * field fires at :00, :07 … :56 and then :00 again, four minutes later, so
 * "every 7 minutes" would be wrong once an hour.
 */
function stepOf(field: string, size: number): number | null {
  const match = /^(?:\*|0)\/(\d+)$/.exec(field);
  if (!match) return null;
  const step = Number(match[1]);
  return step >= 1 && step < size && size % step === 0 ? step : null;
}

function dayOrdinal(token: string): number | null {
  const named = Object.hasOwn(DAY_NAMES, token.toLowerCase())
    ? DAY_NAMES[token.toLowerCase()]
    : undefined;
  return named ?? numberIn(token, 1, 7);
}

/** The days a day-of-week field names, Sunday as 1; `null` for anything else. */
function daysOf(field: string): number[] | null {
  const days = new Set<number>();
  for (const token of field.split(",")) {
    const range = /^([a-z0-9]+)-([a-z0-9]+)$/i.exec(token);
    if (range) {
      const from = dayOrdinal(range[1]!);
      const to = dayOrdinal(range[2]!);
      if (from === null || to === null || from > to) return null;
      for (let day = from; day <= to; day += 1) days.add(day);
    } else {
      const day = dayOrdinal(token);
      if (day === null) return null;
      days.add(day);
    }
  }
  return [...days].sort((a, b) => a - b);
}

function monthOf(field: string): number | null {
  const lower = field.toLowerCase();
  const short = (MONTH_NAMES as readonly string[]).indexOf(lower);
  if (short !== -1) return short + 1;
  const full = (MONTH_FULL as readonly string[]).indexOf(lower);
  if (full !== -1) return full + 1;
  return numberIn(field, 1, 12);
}

/** Up to four hours, as a comma list of plain numbers. */
function hoursOf(field: string): number[] | null {
  const tokens = field.split(",");
  if (tokens.length > 4) return null;
  const hours: number[] = [];
  for (const token of tokens) {
    const hour = numberIn(token, 0, 23);
    if (hour === null) return null;
    if (!hours.includes(hour)) hours.push(hour);
  }
  return hours.sort((a, b) => a - b);
}

/** `a`, `a and b`, `a, b and c`. */
function listOf(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

function ordinal(day: number): string {
  const tens = day % 100;
  if (tens >= 11 && tens <= 13) return `${day}th`;
  return `${day}${["th", "st", "nd", "rd"][day % 10] ?? "th"}`;
}

function daysPhrase(days: readonly number[]): string {
  const key = days.join(",");
  if (key === "2,3,4,5,6") return "Weekdays";
  if (key === "1,7") return "Weekends";
  if (days.length === 1) return DAY_PLURAL[days[0]!] ?? "";
  const first = days[0]!;
  const last = days[days.length - 1]!;
  const contiguous = last - first === days.length - 1;
  if (contiguous && days.length >= 3) return `${DAY_SHORT[first]} to ${DAY_SHORT[last]}`;
  return listOf(days.map((day) => DAY_SHORT[day] ?? ""));
}

/** The words for an expression, and whether they name a time of day. */
export interface CronWords {
  words: string;
  /** The words name a wall-clock time, so which clock it is read in matters. */
  wallClock: boolean;
}

/**
 * Words for the common shapes of cron expression, or `null`.
 *
 * Understood: every minute or every few minutes, and every hour or every few
 * hours at a minute past, when the step divides the hour or the day evenly; and
 * a time of day (up to four hours at one minute) every
 * day, on chosen days of the week, monthly on a day, or yearly on a date.
 * Anything else, including a seconds field other than `0` and a year other
 * than `*`, is left to the expression.
 */
export function describeCron(expression: string): CronWords | null {
  const fields = expression.trim().split(/\s+/);
  let parts: string[];
  if (fields.length === 5) parts = ["0", ...fields];
  else if (fields.length === 6) parts = fields;
  else if (fields.length === 7 && fields[6] === "*") parts = fields.slice(0, 6);
  else return null;
  const [second, minute, hour, dom, month, dow] = parts as [
    string,
    string,
    string,
    string,
    string,
    string,
  ];
  if (second !== "0") return null;
  const dayless = any(dom) && any(month) && any(dow);

  if (minute === "*" && any(hour) && dayless) return { words: "Every minute", wallClock: false };
  const everyMinutes = stepOf(minute, 60);
  if (everyMinutes !== null && any(hour) && dayless) {
    const words = everyMinutes === 1 ? "Every minute" : `Every ${everyMinutes} minutes`;
    return { words, wallClock: false };
  }

  // From here on the words name a minute past the hour, which is a wall-clock
  // fact: read in UTC, `30 * * * *` is on the hour where the offset is a half.
  const at = numberIn(minute, 0, 59);
  if (at === null) return null;
  const past = at === 0 ? ", on the hour" : ` at :${pad(at)}`;
  if (any(hour) && dayless) return { words: `Every hour${past}`, wallClock: true };
  const everyHours = stepOf(hour, 24);
  if (everyHours !== null) {
    if (!dayless) return null;
    const words = everyHours === 1 ? `Every hour${past}` : `Every ${everyHours} hours${past}`;
    return { words, wallClock: true };
  }

  const hours = hoursOf(hour);
  if (hours === null) return null;
  const times = listOf(hours.map((each) => `${pad(each)}:${pad(at)}`));

  if (any(dom) && any(month)) {
    if (any(dow)) return { words: `Every day at ${times}`, wallClock: true };
    const days = daysOf(dow);
    if (days === null) return null;
    if (days.length === 7) return { words: `Every day at ${times}`, wallClock: true };
    return { words: `${daysPhrase(days)} at ${times}`, wallClock: true };
  }
  if (!any(dow)) return null;
  const day = numberIn(dom, 1, 31);
  if (day === null) return null;
  if (any(month)) return { words: `Monthly on the ${ordinal(day)} at ${times}`, wallClock: true };
  const which = monthOf(month);
  if (which === null) return null;
  return { words: `Every year on ${day} ${monthName(which)} at ${times}`, wallClock: true };
}

/**
 * Words for a cron expression read in `clock`, or `null`. A time of day read in
 * UTC says so; one read in local time does not, because local time is what a
 * bare time means to the person reading it.
 */
export function cronWords(expression: string, clock: string | null): string | null {
  const described = describeCron(expression);
  if (described === null) return null;
  return described.wallClock && clock === "utc" ? `${described.words} UTC` : described.words;
}

/** The shape every cadence on this screen shares, sent or stored. */
export interface CadenceShape {
  kind: string;
  seconds: number | null;
  expression: string | null;
  clock: string | null;
}

/** Words for any cadence, or `null` when there are none certain enough to say. */
export function cadenceWords(cadence: CadenceShape): string | null {
  switch (cadence.kind) {
    case "every":
      return cadence.seconds === null ? null : intervalWords(cadence.seconds);
    case "cron":
      return cadence.expression === null ? null : cronWords(cadence.expression, cadence.clock);
    case "once":
      return "Once";
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Presets and the preview's heading
// ---------------------------------------------------------------------------

/** Intervals offered as one-press choices beside the seconds field. */
export const INTERVAL_PRESETS: readonly { seconds: number; words: string }[] = [
  900, 3_600, 21_600, 86_400,
].map((seconds) => ({ seconds, words: intervalWords(seconds) ?? `Every ${seconds} seconds` }));

/**
 * Cron expressions offered as one-press choices beside the expression field.
 * Each is labelled with its own words, so a preset can never say one thing and
 * fill in another.
 */
export const CRON_PRESETS: readonly { expression: string; words: string }[] = [
  DEFAULT_EXPRESSION,
  "0 * * * *",
  "30 8 * * MON",
  "0 9 1 * *",
].map((expression) => ({ expression, words: cronWords(expression, "local") ?? expression }));

/**
 * The heading of a valid preview: words for the cadence, and the runtime's
 * exact description beneath them when the words are not the description.
 * `null` while there is nothing valid to describe.
 */
export function previewWords(
  form: CadenceForm,
  verdict: CadenceVerdict,
): { words: string; exact: string | null } | null {
  if (verdict.state !== "valid") return null;
  if (form.kind === "once") {
    const text = verdict.description;
    return { words: text.charAt(0).toUpperCase() + text.slice(1), exact: null };
  }
  const local = cadenceInput(form);
  const words = local.ok ? cadenceWords(local.value) : null;
  if (words === null) return { words: verdict.description, exact: null };
  return { words, exact: verdict.description };
}
