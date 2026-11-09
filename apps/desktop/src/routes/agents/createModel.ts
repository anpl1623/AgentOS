/**
 * What the create-agent form decides: what it opens with, whether it holds
 * anything to lose, what it sends, and how far through it the operator is.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { CreateAgentInput } from "../../bindings/CreateAgentInput";
import type { ToolView } from "../../bindings/ToolView";

/** What the navigation guard says while the create form is dirty. */
export const CREATE_GUARD = "This new agent has not been created yet. Leave and discard it?";

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

/**
 * What is wrong with the name as typed, or `null` when nothing the screen can
 * see is.
 *
 * Only a name already taken is caught here, against the list on screen: the
 * runtime refuses a duplicate too, and saying so beside the field is kinder
 * than saying so after the click. An empty name is not an error to shout
 * about; the Create button simply waits for one. Compared as the runtime will
 * store it, trimmed and with case kept.
 */
export function nameProblem(name: string, agents: readonly AgentSummary[] | null): string | null {
  const trimmed = name.trim();
  if (trimmed === "" || agents === null) return null;
  return agents.some((agent) => agent.name === trimmed)
    ? `There is already an agent named “${trimmed}”.`
    : null;
}

/** Which of the form's three steps have what they need. */
export interface CreateProgress {
  identity: boolean;
  model: boolean;
  tools: boolean;
  /** Whether the form can be sent: a usable name and a model. */
  ready: boolean;
}

/**
 * How far through the form the operator is, for the step markers and the
 * Create button.
 *
 * Tools are not required: an agent with none is a valid, if quiet, agent. The
 * tools step is marked done only when at least one is granted, so the marker
 * does not claim a choice nobody made.
 */
export function createProgress(
  form: CreateForm,
  agents: readonly AgentSummary[] | null,
): CreateProgress {
  const identity = form.name.trim() !== "" && nameProblem(form.name, agents) === null;
  const model = form.model.trim() !== "";
  return { identity, model, tools: form.tools.length > 0, ready: identity && model };
}

/** The catalogue's tools under one domain. */
export interface ToolGroup {
  domain: string;
  tools: ToolView[];
}

/**
 * The catalogue grouped by domain, each group in the catalogue's own order and
 * the groups in the order their first tool appears, so the form reads the way
 * the catalogue does.
 */
export function groupCatalogue(catalogue: readonly ToolView[]): ToolGroup[] {
  const groups: ToolGroup[] = [];
  for (const tool of catalogue) {
    const group = groups.find((each) => each.domain === tool.domain);
    if (group) group.tools.push(tool);
    else groups.push({ domain: tool.domain, tools: [tool] });
  }
  return groups;
}
