/**
 * What the policy section decides: how the engine's rules are laid out, what a
 * draft holds, and what a check says.
 *
 * The rules are the engine's own descriptions. Nothing here parses YAML or
 * second-guesses the engine; it only lays out what the engine said.
 */

import type { PolicyView } from "../../bindings/PolicyView";
import type { VerdictTone } from "../../components/status";

/** A rule, split into what it names and what it does. */
export interface RuleLine {
  capability: string;
  /** The effect and its resources, or `""` for a line with no `=>`. */
  effect: string;
}

/**
 * Split one of the engine's rule descriptions on its first `" => "`.
 *
 * The description is the engine's; this only lays it out in two columns. A
 * line without the separator is kept whole in the first column rather than
 * dropped, so a change in the engine's wording cannot make a rule vanish from
 * the screen.
 */
export function splitRule(rule: string): RuleLine {
  const at = rule.indexOf(" => ");
  if (at === -1) return { capability: rule, effect: "" };
  return { capability: rule.slice(0, at), effect: rule.slice(at + 4) };
}

/**
 * The tone a rule's effect is drawn in, read from its first word.
 *
 * `allow` is ok, `ask` the attention hue (each call will reach a person), and
 * `deny` the refusal tone. Anything else — an effect worded in a way this build
 * has not seen — is neutral rather than guessed at, and the words are still
 * shown as the engine wrote them.
 */
export function ruleTone(effect: string): VerdictTone | "warn" {
  const word = effect.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
  switch (word) {
    case "allow":
      return "ok";
    case "ask":
    case "asks":
      return "warn";
    case "deny":
      return "blocked";
    default:
      return "neutral";
  }
}

/** A rule's effect, split into its verb and the resources it names. */
export interface EffectParts {
  /** The first word — `allow`, `ask`, `deny` — or `""` for an empty effect. */
  verb: string;
  /** Everything after it, as the engine wrote it. */
  rest: string;
}

/**
 * Split an effect after its first word, so the verb can be drawn as a chip in
 * its tone and the resources beside it as written. Nothing is dropped: the
 * two parts, joined by a space, are the effect.
 */
export function effectParts(effect: string): EffectParts {
  const trimmed = effect.trim();
  const at = trimmed.search(/\s/);
  if (at === -1) return { verb: trimmed, rest: "" };
  return { verb: trimmed.slice(0, at), rest: trimmed.slice(at).trim() };
}

/**
 * A policy being edited: the text, and the stored version it started from.
 *
 * The base travels with the draft so that a save made elsewhere — the CLI, or
 * another window — while this one was open is noticed rather than silently
 * overwritten by text written against the older version.
 */
export interface PolicyDraft {
  document: string;
  baseVersion: number;
  baseDocument: string;
}

/** Start editing `policy`. */
export function beginPolicyDraft(policy: PolicyView): PolicyDraft {
  return { document: policy.document, baseVersion: policy.version, baseDocument: policy.document };
}

/** Whether a draft holds anything that saving would change. */
export function policyDirty(draft: PolicyDraft | null): boolean {
  return draft !== null && draft.document !== draft.baseDocument;
}

/**
 * Whether the stored policy has moved on since the draft began.
 *
 * Only worth saying when the draft is dirty: an unchanged draft has nothing of
 * the operator's to lose, and the editor can simply be reopened on the new
 * version.
 */
export function policySuperseded(draft: PolicyDraft | null, policy: PolicyView | null): boolean {
  return draft !== null && policy !== null && policy.version !== draft.baseVersion;
}

/** What the navigation guard says while the policy draft is dirty. */
export const POLICY_GUARD = "This policy has unsaved changes. Leave and discard them?";

/**
 * What a successful check says: the default and how many rules the engine
 * read, which is enough to notice a rule that did not parse as one.
 */
export function checkedText(summary: Pick<PolicyView, "default_effect" | "rules">): string {
  const count = summary.rules.length;
  const rules = count === 1 ? "1 rule" : `${count} rules`;
  return `Valid · default ${summary.default_effect} · ${rules}`;
}

/**
 * How many lines the editor's gutter numbers.
 *
 * Every line the text area draws, including an empty last one after a final
 * newline: the caret can sit there, so the gutter has a number for it.
 */
export function lineCount(text: string): number {
  let lines = 1;
  for (const char of text) if (char === "\n") lines += 1;
  return lines;
}
