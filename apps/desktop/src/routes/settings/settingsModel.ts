/**
 * What the settings screen decides, apart from how it draws it.
 *
 * The decisions are pure functions, so a test fails when one of them changes,
 * and they live in one module per concern beside the section that draws them:
 *
 * - `catalogueModel.ts`: who holds each tool, and which tools a filter keeps.
 * - `schedulerModel.ts`: the pacing fields, and what the scheduler panel
 *   claims about the work it would find.
 * - `auditModel.ts`: how a verification of the audit chain is worded.
 * - `credentialsModel.ts` and `integrationsModel.ts`: how a stored
 *   credential and a bound account are shown, and when their forms can be sent.
 * - `appearanceModel.ts`, `overviewModel.ts` and `sectionsModel.ts`: the
 *   theme cards, the tiles at the top, and the in-page section navigation.
 *
 * This module holds the wording they share. The components only render what
 * these return.
 */

/**
 * A moment relative to `now`, either side of it: `in 4m`, `2h ago`.
 *
 * Whole units, rounded towards zero, so a schedule due in eleven and a half
 * minutes is never promised in twelve. Within a minute either way it is `now`,
 * and a time that cannot be read says so rather than printing `NaN`.
 */
export function relative(iso: string, now: number): string {
  const offset = Date.parse(iso) - now;
  if (!Number.isFinite(offset)) return "at an unreadable time";
  const length = Math.abs(offset);
  if (length < 60_000) return "now";
  const minutes = Math.floor(length / 60_000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  const amount = days > 0 ? `${days}d` : hours > 0 ? `${hours}h` : `${minutes}m`;
  return offset > 0 ? `in ${amount}` : `${amount} ago`;
}

/**
 * A runtime message as a sentence: first letter capitalised, closing stop.
 *
 * The runtime writes its refusals to follow a colon, in lower case. Shown on
 * their own in a banner, they read better as a sentence; the words are left
 * exactly as written.
 */
export function asSentence(message: string): string {
  const trimmed = message.trim();
  if (trimmed === "") return trimmed;
  const capitalised = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capitalised) ? capitalised : `${capitalised}.`;
}

/** `1 break`, `2 breaks`: a count with its noun, singular when it is one. */
export function counted(count: number, one: string, many: string): string {
  return count === 1 ? `1 ${one}` : `${count} ${many}`;
}
