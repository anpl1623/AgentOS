/**
 * What the tools and grants section decides.
 *
 * Nothing here decides what an agent may do. Reach comes from the permission
 * engine's grant report, risk from the tool catalogue; this module only joins
 * them, so that the two configurations which produce an agent that looks
 * capable and refuses everything — a grant no rule allows, and a grant above
 * the risk ceiling — are on screen beside the grant itself.
 */

import type { PolicyView } from "../../bindings/PolicyView";
import type { ToolGrantView } from "../../bindings/ToolGrantView";
import type { ToolView } from "../../bindings/ToolView";
import { RISK_ORDER, type VerdictTone, compareRisk } from "../../components/status";

/** How far the engine says one capability reaches, or `null` before it has said. */
export interface CapabilityLine {
  capability: string;
  reach: string | null;
}

/**
 * Why every call a granted tool makes will be refused, when that can be read
 * from what the screen holds.
 *
 * - `unknown`: the registry has no tool by that name.
 * - `ceiling`: the tool's baseline risk is above the policy's ceiling.
 * - `no-rule`: the engine reports every capability the tool declares as
 *   denied.
 */
export type DeadGrant = "unknown" | "ceiling" | "no-rule";

/** One granted tool, as the tools section draws it. */
export interface GrantRow {
  tool: string;
  /** The catalogue's domain, or the name's prefix when the catalogue lacks it. */
  domain: string;
  /** Baseline risk, or `null` when the catalogue has not loaded or lacks it. */
  risk: string | null;
  /** Whether its output can be attacker-controlled. */
  external: boolean;
  description: string | null;
  /** From the grant report when it has loaded, else from the catalogue. */
  capabilities: CapabilityLine[];
  dead: DeadGrant | null;
}

/**
 * Join an agent's granted tool names with the catalogue and the grant report.
 *
 * Either source may still be loading or may have failed, so each is optional
 * and a row says only what the sources present can support: no `unknown` until
 * one of them has answered, and no `no-rule` without the report.
 *
 * The ceiling is checked here, from the policy and the catalogue, because it
 * is the more specific reason: the engine reports a tool above the ceiling as
 * denied too, and calling that "no rule allows this" would send a person to
 * add a rule that cannot help. A risk level this build does not recognise is
 * not compared, so the screen never claims a ceiling refusal it cannot
 * establish; the report still says whether the engine denies it.
 */
export function grantRows(
  granted: readonly string[],
  catalogue: readonly ToolView[] | null,
  report: readonly ToolGrantView[] | null,
  policy: PolicyView | null,
): GrantRow[] {
  const tools = new Map((catalogue ?? []).map((tool) => [tool.name, tool]));
  const reports = new Map((report ?? []).map((grant) => [grant.tool, grant]));

  return granted.map((name) => {
    const tool = tools.get(name);
    const grant = reports.get(name);
    const unknown =
      (grant !== undefined && !grant.registered) || (catalogue !== null && tool === undefined);

    const capabilities: CapabilityLine[] = grant
      ? grant.capabilities.map(({ capability, reach }) => ({ capability, reach }))
      : (tool?.capabilities ?? []).map((capability) => ({ capability, reach: null }));

    let dead: DeadGrant | null = null;
    if (unknown) {
      dead = "unknown";
    } else if (tool && aboveCeiling(tool.risk, policy?.max_risk ?? null)) {
      dead = "ceiling";
    } else if (grant && grant.capabilities.length > 0 && grant.capabilities.every(isDenied)) {
      dead = "no-rule";
    }

    return {
      tool: name,
      domain: tool?.domain ?? (name.split(".")[0] || name),
      risk: tool?.risk ?? null,
      external: tool?.returns_untrusted_data ?? false,
      description: tool?.description ?? null,
      capabilities,
      dead,
    };
  });
}

function isDenied({ reach }: { reach: string }): boolean {
  return reach === "denied";
}

/** The chip and the sentence for a grant whose every call will be refused. */
export interface DeadGrantText {
  label: string;
  sentence: string;
}

/**
 * What a dead grant says.
 *
 * Each sentence names the cause and the consequence, and nothing it cannot
 * establish: `no-rule` is the engine's verdict on every declared capability,
 * and is worded as that, not as a claim about which rule is missing.
 */
export function deadGrantText(row: GrantRow, policy: PolicyView | null): DeadGrantText | null {
  switch (row.dead) {
    case null:
      return null;
    case "unknown":
      return {
        label: "Unknown tool",
        sentence: "This installation has no tool by that name, so the agent can never call it.",
      };
    case "ceiling":
      return {
        label: "Above the risk ceiling",
        sentence:
          `Its ${row.risk ?? "baseline"} risk is above this policy's ceiling of ` +
          `${policy?.max_risk ?? "its ceiling"} — every call will be refused.`,
      };
    case "no-rule":
      return {
        label: "No rule allows this",
        sentence: "No rule allows this — every call will be refused.",
      };
  }
}

/** Whether `risk` is strictly above `ceiling`, when both are levels this build knows. */
export function aboveCeiling(risk: string, ceiling: string | null): boolean {
  if (ceiling === null) return false;
  const known = RISK_ORDER as readonly string[];
  if (!known.includes(risk) || !known.includes(ceiling)) return false;
  return compareRisk(risk, ceiling) > 0;
}

/** A reach as the chip says it. */
export function reachLabel(reach: string): string {
  switch (reach) {
    case "allowed":
      return "Allowed";
    case "scoped":
      return "Scoped";
    case "asks":
      return "Asks";
    case "denied":
      return "Denied";
    default:
      return reach;
  }
}

/**
 * The tone a reach is drawn in.
 *
 * `asks` is not `ok`: a capability that reaches a person every time is a
 * question the operator will be asked, not a grant. `denied` takes the refusal
 * tone, because it is the policy working as written, if not as intended. A
 * reach this build does not know is neutral rather than guessed at.
 */
export function reachTone(reach: string): VerdictTone | "warn" {
  switch (reach) {
    case "allowed":
      return "ok";
    case "scoped":
      return "neutral";
    case "asks":
      return "warn";
    case "denied":
      return "blocked";
    default:
      return "neutral";
  }
}

/**
 * The key beside the grants: each of the engine's four answers, the chip it
 * is drawn as, and what it means. The chips are built from `reachLabel` and
 * `reachTone`, so the key cannot disagree with the rows it explains.
 */
export const REACH_LEGEND: readonly { reach: string; meaning: string }[] = [
  { reach: "allowed", meaning: "allowed at the tool’s usual risk" },
  { reach: "scoped", meaning: "some resources may be allowed or asked about" },
  { reach: "asks", meaning: "every call waits for approval" },
  { reach: "denied", meaning: "every call is refused" },
];

/** The figures the overview gives for an agent's grants. */
export interface GrantTally {
  granted: number;
  /** Grants whose every call will be refused. */
  refused: number;
  /** The most dangerous baseline risk among the grants the catalogue knows. */
  highest: string | null;
}

/** Count `rows` for the overview. */
export function grantTally(rows: readonly GrantRow[]): GrantTally {
  let highest: string | null = null;
  for (const row of rows) {
    if (row.risk !== null && (highest === null || compareRisk(row.risk, highest) > 0)) {
      highest = row.risk;
    }
  }
  return {
    granted: rows.length,
    refused: rows.filter((row) => row.dead !== null).length,
    highest,
  };
}

/** Where the engine's grant report stands: still being read, read, or failed. */
export type ReportState = "reading" | "read" | "failed";

/** The overview's refused-grants figure: its number, the words under it, and its hue. */
export interface RefusedFigure {
  /** `null` draws the tile's pending mark: nothing found yet, and nothing ruled out. */
  value: number | null;
  hint: string;
  tone: "accent" | "warn" | "danger";
}

/**
 * The refused-grants figure.
 *
 * Only the report can say that no rule allows a grant, so a zero is a claim
 * the screen may make only once the report has been read: before then, or
 * after it failed, "none" would be a statement of safety nobody checked. What
 * the catalogue and the ceiling alone establish is still counted and shown
 * while the report is outstanding, since those refusals are certain. A failed
 * read takes the attention hue when it leaves the figure unknown.
 */
export function refusedFigure(rows: readonly GrantRow[], report: ReportState): RefusedFigure {
  if (rows.length === 0) return { value: 0, hint: "no tools granted", tone: "accent" };
  const refused = rows.filter((row) => row.dead !== null).length;
  if (refused > 0) {
    const hint =
      report === "failed"
        ? "every call will be refused; the policy's reach could not be read"
        : "every call will be refused";
    return { value: refused, hint, tone: "danger" };
  }
  switch (report) {
    case "read":
      return { value: 0, hint: "none found", tone: "accent" };
    case "reading":
      return { value: null, hint: "reading the policy…", tone: "accent" };
    case "failed":
      return { value: null, hint: "the policy's reach could not be read", tone: "warn" };
  }
}
