/**
 * What the agents screen decides, apart from how it draws it.
 *
 * Nothing here decides what an agent may do. Reach comes from the permission
 * engine's grant report, risk from the tool catalogue, and rules from the
 * engine's own descriptions; this module only joins them, so that the two
 * configurations which produce an agent that looks capable and refuses
 * everything — a grant no rule allows, and a grant above the risk ceiling —
 * are on screen beside the grant itself.
 */

import type { AgentSummary } from "../bindings/AgentSummary";
import type { CreateAgentInput } from "../bindings/CreateAgentInput";
import type { MemoryView } from "../bindings/MemoryView";
import type { PolicyView } from "../bindings/PolicyView";
import type { ToolGrantView } from "../bindings/ToolGrantView";
import type { ToolView } from "../bindings/ToolView";
import { RISK_ORDER, type VerdictTone, compareRisk } from "../components/status";

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/**
 * The agents whose name, provider or model contains `query`, ignoring case.
 *
 * `provider/model` is matched as one string as well, because that is how the
 * row writes it, and a person types what they read.
 */
export function filterAgents(agents: readonly AgentSummary[], query: string): AgentSummary[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...agents];
  return agents.filter((agent) =>
    [agent.name, agent.provider, agent.model, `${agent.provider}/${agent.model}`].some((field) =>
      field.toLowerCase().includes(needle),
    ),
  );
}

/**
 * The most dangerous baseline risk among the tools an agent has been granted,
 * or `null` when none of them is in the catalogue.
 *
 * A granted name the catalogue does not know is left out rather than ranked:
 * it can never be called, so it adds no reach. A level this build does not
 * recognise ranks above `critical`, for the reason `compareRisk` gives.
 */
export function highestRisk(
  granted: readonly string[],
  catalogue: readonly ToolView[],
): string | null {
  const risks = new Map(catalogue.map((tool) => [tool.name, tool.risk]));
  let highest: string | null = null;
  for (const name of granted) {
    const risk = risks.get(name);
    if (risk === undefined) continue;
    if (highest === null || compareRisk(risk, highest) > 0) highest = risk;
  }
  return highest;
}

// ---------------------------------------------------------------------------
// Grants
// ---------------------------------------------------------------------------

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

/** One granted tool, as the tools panel draws it. */
export interface GrantRow {
  tool: string;
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

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

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

/** What the navigation guard says while the create form is dirty. */
export const CREATE_GUARD = "This new agent has not been created yet. Leave and discard it?";

// ---------------------------------------------------------------------------
// Create form
// ---------------------------------------------------------------------------

/** Whether this model can be shown images: yes, no, or the provider's default. */
export type Vision = "default" | "yes" | "no";

/** The create form, held as one object so one draft and one guard cover it. */
export interface CreateForm {
  name: string;
  instructions: string;
  provider: string;
  model: string;
  baseUrl: string;
  vision: Vision;
  tools: string[];
}

/** The providers the form offers. */
export const PROVIDERS = ["anthropic", "openai", "ollama", "mock"] as const;

/** The form as it opens. */
export function newCreateForm(): CreateForm {
  return {
    name: "",
    instructions: "Complete the operator's objective carefully and report what you did.",
    provider: "anthropic",
    model: "claude-opus-5",
    baseUrl: "",
    vision: "default",
    // The CLI's read-only default: search plans only `list` and `read`.
    tools: ["filesystem.read", "filesystem.list", "filesystem.search"],
  };
}

/**
 * Whether the form differs from how it opened.
 *
 * Tools are compared as a set: ticking one and unticking it again leaves
 * nothing to lose, whatever order the array ends up in.
 */
export function createFormDirty(form: CreateForm | null): boolean {
  if (form === null) return false;
  const fresh = newCreateForm();
  const sameTools =
    form.tools.length === fresh.tools.length &&
    fresh.tools.every((tool) => form.tools.includes(tool));
  return (
    !sameTools ||
    form.name !== fresh.name ||
    form.instructions !== fresh.instructions ||
    form.provider !== fresh.provider ||
    form.model !== fresh.model ||
    form.baseUrl !== fresh.baseUrl ||
    form.vision !== fresh.vision
  );
}

/** Whether the base URL field applies to `provider`. */
export function takesBaseUrl(provider: string): boolean {
  return provider === "openai" || provider === "ollama";
}

/** Grant `tool` if it is not granted, or withdraw it if it is. */
export function toggleTool(tools: readonly string[], tool: string): string[] {
  return tools.includes(tool) ? tools.filter((name) => name !== tool) : [...tools, tool];
}

/**
 * What the form sends.
 *
 * A base URL is sent only for a provider that takes one: a URL typed for
 * `ollama` and left behind on switching to `anthropic` is not on screen any
 * more, and must not be applied where nobody can see it.
 */
export function createInput(form: CreateForm): CreateAgentInput {
  const baseUrl = form.baseUrl.trim();
  return {
    name: form.name.trim(),
    instructions: form.instructions,
    provider: form.provider,
    model: form.model.trim(),
    base_url: takesBaseUrl(form.provider) && baseUrl !== "" ? baseUrl : null,
    vision: form.vision === "default" ? null : form.vision === "yes",
    tools: [...form.tools],
  };
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/** The kinds of memory the runtime keeps, in the order the filter offers them. */
export const MEMORY_KINDS = [
  "fact",
  "decision",
  "preference",
  "task_history",
  "observation",
] as const;

/** The memories of `kind` (or every kind, for `null`) whose content or source contains `query`. */
export function filterMemories(
  memories: readonly MemoryView[],
  query: string,
  kind: string | null,
): MemoryView[] {
  const needle = query.trim().toLowerCase();
  return memories.filter(
    (memory) =>
      (kind === null || memory.kind === kind) &&
      (needle === "" ||
        memory.content.toLowerCase().includes(needle) ||
        memory.source.toLowerCase().includes(needle)),
  );
}

/**
 * A confidence as the row prints it: two places at most, no trailing zeros.
 *
 * Clamped to the range the runtime stores, so a value that drifted outside it
 * on the wire is not printed as a confidence nobody could have set.
 */
export function confidenceLabel(confidence: number): string {
  if (!Number.isFinite(confidence)) return "confidence unknown";
  const clamped = Math.min(1, Math.max(0, confidence));
  return `confidence ${Number(clamped.toFixed(2))}`;
}

/** What the screen does when a memory edit is saved: `null` sends no change. */
export interface MemoryRevision {
  content: string;
  confidence: number | undefined;
}

/**
 * The revision an edit amounts to, or `null` when there is nothing to send.
 *
 * Empty content is refused here rather than sent: forgetting is its own
 * action, with its own question, and an emptied note is not a way round it.
 * The confidence is sent only when it changed, so an edit to the wording does
 * not restate a number the operator never touched.
 */
export function memoryRevision(
  memory: MemoryView,
  content: string,
  confidence: number,
): MemoryRevision | null {
  if (content.trim() === "") return null;
  const confidenceChanged = Math.abs(confidence - memory.confidence) > 1e-9;
  if (content === memory.content && !confidenceChanged) return null;
  return { content, confidence: confidenceChanged ? confidence : undefined };
}
