/**
 * The audit log panel: what a full verification of the chain found, and how
 * that is worded, coloured and announced.
 */

import type { AuditVerificationView } from "../../bindings/AuditVerificationView";
import { counted, relative } from "./settingsModel";

/** A finished verification of the whole chain, as the screen remembers it. */
export interface Verification {
  /** When it finished, as an ISO time. */
  at: string;
  /**
   * Everything found, chain breaks first and coverage gaps after; empty only
   * when the chain is intact and complete.
   */
  problems: string[];
}

/** A finished verification with what it found kept apart by kind, as the screen lists it. */
export interface AuditFindings extends Verification {
  /** Where the hash chain does not verify. */
  breaks: string[];
  /** Where the chain verifies but the story it holds is incomplete. */
  gaps: string[];
}

/**
 * What the full check answered, finished at `at`.
 *
 * A gap counts as a problem as a break does. A chain that verifies but holds
 * a tool call with no end, or a delegated run with no records, is consistent
 * and still not the whole story, and "intact" must not be said of it.
 */
export function auditFindings(at: string, view: AuditVerificationView): AuditFindings {
  return {
    at,
    breaks: [...view.breaks],
    gaps: [...view.gaps],
    problems: [...view.breaks, ...view.gaps],
  };
}

/** What the full check found, each kind kept apart. */
export type Found = Pick<AuditFindings, "breaks" | "gaps">;

/**
 * What was found, named by kind: `1 break`, `2 coverage gaps`, or both.
 * `null` when nothing was.
 *
 * Never one count of "problems". A break means a record was altered; a gap
 * means the records that are there do not account for something. Summed, a
 * log with only a gap would read like a chain that failed to verify.
 */
export function findingsSummary(found: Found): string | null {
  const parts = [
    found.breaks.length > 0 ? counted(found.breaks.length, "break", "breaks") : null,
    found.gaps.length > 0 ? counted(found.gaps.length, "coverage gap", "coverage gaps") : null,
  ].filter((part): part is string => part !== null);
  return parts.length === 0 ? null : parts.join(", ");
}

/**
 * The visible line under the button: when the chain was last checked, and
 * what was found. Never checked this session says so, so the absence of a
 * result is not taken for a clean one.
 */
export function verificationLine(last: AuditFindings | null, now: number): string {
  if (last === null) return "not verified since the window opened";
  return `verified ${relative(last.at, now)} · ${findingsSummary(last) ?? "intact"}`;
}

/** The sentence a screen reader hears when a verification finishes. */
export function verificationAnnouncement(found: Found): string {
  const breaks = counted(found.breaks.length, "break", "breaks");
  const gaps = counted(found.gaps.length, "coverage gap", "coverage gaps");
  if (found.breaks.length > 0) {
    return found.gaps.length > 0
      ? `Audit chain verification found ${breaks}, and ${gaps}.`
      : `Audit chain verification found ${breaks}.`;
  }
  return found.gaps.length > 0
    ? `Audit chain verified, but its records have ${gaps}.`
    : "Audit chain verified intact.";
}

/**
 * The sentence above the coverage gaps. It may say the chain verifies only
 * when no break is listed above it.
 */
export function gapsNote(found: Found): string {
  return found.breaks.length > 0
    ? "Separately from the breaks above, the records do not tell the whole story."
    : "The chain verifies, but the records it holds do not tell the whole story.";
}

/** The hue a verification is drawn in. */
export type VerificationTone = "ok" | "warn" | "danger";

/**
 * How loud a result is. A break is a failure: a record was altered, and the
 * chain itself is in doubt. Gaps alone are attention, not failure — the
 * chain verifies and the story is incomplete — so they are drawn in the
 * attention hue, never the failure one, for the same reason they are never
 * counted as breaks. Only a result with neither is drawn as well.
 */
export function verificationTone(found: Found): VerificationTone {
  if (found.breaks.length > 0) return "danger";
  return found.gaps.length > 0 ? "warn" : "ok";
}

/**
 * The result in a word or two, for a tile and the seal beside the button:
 * `Intact`, `1 break`, `2 gaps`. Breaks are named first and alone, since a
 * break outranks any number of gaps; the full count is on the line below.
 */
export function verificationWord(last: Found | null): string {
  if (last === null) return "Unverified";
  if (last.breaks.length > 0) return counted(last.breaks.length, "break", "breaks");
  if (last.gaps.length > 0) return counted(last.gaps.length, "gap", "gaps");
  return "Intact";
}
