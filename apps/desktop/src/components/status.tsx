/**
 * Chips that say what something *is* or what *happened* to it.
 *
 * Two shapes, deliberately unlike each other. `.risk` is a property of a tool
 * or a call, known before anything runs. `.verdict` is an outcome, known after.
 * When both used the same red pill, a correctly refused call and a crashed tool
 * read identically, in a product whose claim is that refusals are visible.
 *
 * Every chip carries a visually hidden prefix naming what it describes. Colour
 * and shape tell a sighted reader which chip is which; a screen reader hears
 * only the words, and "high, denied" beside "high risk, outcome denied" is the
 * difference between a guess and a fact.
 */

import { humanise } from "../sdk/format";

/** Risk levels from least to most dangerous, matching the runtime's `RiskLevel`. */
export const RISK_ORDER = ["none", "low", "medium", "high", "critical"] as const;

/** A risk level the runtime is known to emit. */
export type RiskLevel = (typeof RISK_ORDER)[number];

/**
 * Orders two risk levels, least dangerous first, for use with `Array.sort`.
 *
 * A level this build does not recognise ranks above `critical`. The wire is a
 * string, and a runtime newer than the window may one day send a level the
 * window has never heard of; sorting it to the dangerous end means a
 * descending list puts it in front of the operator rather than burying it
 * beneath the `none` rows.
 */
export function compareRisk(a: string, b: string): number {
  return riskRank(a) - riskRank(b);
}

function riskRank(level: string): number {
  const rank = (RISK_ORDER as readonly string[]).indexOf(level);
  return rank === -1 ? RISK_ORDER.length : rank;
}

/**
 * The class a risk chip is drawn with.
 *
 * A level this build does not recognise is drawn as `critical`, agreeing with
 * {@link compareRisk}, which sorts it above `critical`. A row sorted to the
 * front as the most dangerous must not wear the quietest chip on screen.
 */
export function riskTone(level: string): RiskLevel {
  return (RISK_ORDER as readonly string[]).includes(level) ? (level as RiskLevel) : "critical";
}

/** The tones a verdict can take. Each is a class in `status.css`. */
export type VerdictTone = "ok" | "danger" | "blocked" | "neutral" | "live";

/**
 * Whether `key` is an own entry of `table`.
 *
 * A bare index would answer `"constructor"` or `"toString"` with a function
 * from the prototype, and that function would be interpolated into a class
 * name. Wire values are strings the window did not choose, so every lookup
 * goes through this.
 */
function known<K extends string, V>(table: Readonly<Record<K, V>>, key: string): key is K {
  return Object.hasOwn(table, key);
}

function lookup<K extends string>(
  table: Readonly<Record<K, VerdictTone>>,
  key: string,
): VerdictTone {
  return known(table, key) ? table[key] : "neutral";
}

/**
 * The full `ToolOutcome` wire vocabulary.
 *
 * Mapped value by value rather than through the runtime's `executed` flag,
 * because that flag answers a different question. A refusal did not execute
 * and neither did a malformed call, but one is the policy doing its job and
 * the other is the model making a mistake; they must not share a colour.
 */
const OUTCOME_TONES = {
  success: "ok",
  failed: "danger",
  timed_out: "danger",
  invalid_arguments: "danger",
  denied: "blocked",
  approval_denied: "blocked",
  cancelled: "neutral",
} as const satisfies Record<string, VerdictTone>;

/** Run states. Every state that is still moving keeps the accent tone. */
const RUN_TONES = {
  idle: "neutral",
  planning: "live",
  executing: "live",
  observing: "live",
  verifying: "live",
  waiting_for_approval: "live",
  recovering: "live",
  completed: "ok",
  failed: "danger",
  cancelled: "neutral",
} as const satisfies Record<string, VerdictTone>;

/**
 * Task statuses.
 *
 * `blocked` is neutral, not the `blocked` tone: a task waiting on another task
 * in its graph has not been refused by anything, and painting it in the colour
 * of a policy denial would claim a decision nobody made.
 */
const TASK_TONES = {
  pending: "neutral",
  blocked: "neutral",
  running: "live",
  succeeded: "ok",
  failed: "danger",
  cancelled: "neutral",
} as const satisfies Record<string, VerdictTone>;

/** The tone for a tool outcome; neutral for anything this build does not know. */
export function outcomeTone(outcome: string): VerdictTone {
  return lookup(OUTCOME_TONES, outcome);
}

/** The tone for a run state; neutral for anything this build does not know. */
export function runTone(state: string): VerdictTone {
  return lookup(RUN_TONES, state);
}

/** The tone for a task status; neutral for anything this build does not know. */
export function taskTone(status: string): VerdictTone {
  return lookup(TASK_TONES, status);
}

function Chip({
  shape,
  tone,
  noun,
  children,
}: {
  shape: "risk" | "verdict";
  tone: string;
  noun: string;
  children: string;
}) {
  return (
    <span className={`${shape} ${tone}`}>
      <span className="visually-hidden">{noun} </span>
      {children}
    </span>
  );
}

/**
 * A wire value as chip text: underscores to spaces, first letter capitalised.
 *
 * Done here rather than with `::first-letter`, which would capitalise the
 * visually hidden prefix instead of the word a sighted reader sees.
 */
function sentence(value: string): string {
  const words = humanise(value);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** A risk level, outlined by severity. The one thing on screen that should shout. */
export function Risk({ level }: { level: string }) {
  return (
    <Chip shape="risk" tone={riskTone(level)} noun="risk">
      {level}
    </Chip>
  );
}

/** What happened to a tool call. */
export function Verdict({ outcome }: { outcome: string }) {
  return (
    <Chip shape="verdict" tone={outcomeTone(outcome)} noun="outcome">
      {sentence(outcome)}
    </Chip>
  );
}

/** A run state. */
export function State({ state }: { state: string }) {
  return (
    <Chip shape="verdict" tone={runTone(state)} noun="run">
      {sentence(state)}
    </Chip>
  );
}

/** A task status. */
export function Status({ status }: { status: string }) {
  return (
    <Chip shape="verdict" tone={taskTone(status)} noun="task">
      {sentence(status)}
    </Chip>
  );
}

/**
 * Whether an agent may be given work.
 *
 * Deliberately not the run-state chip: an *enabled* agent is not a *running*
 * one, and showing "running" beside an idle agent would be a lie about what the
 * machine is doing.
 */
export function Enabled({ status }: { status: string }) {
  const on = status === "enabled";
  return (
    <Chip shape="verdict" tone={on ? "ok" : "neutral"} noun="agent">
      {on ? "Enabled" : "Disabled"}
    </Chip>
  );
}

/**
 * The marker for a run that has read untrusted data.
 *
 * Shown wherever such a run appears, not only on the approval card. A person
 * scanning a list should be able to see which work was influenced by something
 * the operator did not write. The explanation is text rather than a `title`,
 * which a keyboard never reaches and a screen reader may not read.
 */
export function Tainted({ label = "read untrusted data" }: { label?: string }) {
  return (
    <span className="taint">
      <span aria-hidden="true">⚠</span>
      {label}
      <span className="visually-hidden">
        . This run has read data from outside the trust boundary.
      </span>
    </span>
  );
}
