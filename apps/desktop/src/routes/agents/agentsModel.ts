/**
 * What the agents list decides, apart from how it draws it.
 *
 * Nothing in this folder decides what an agent may do. Reach comes from the
 * permission engine's grant report, risk from the tool catalogue, and rules
 * from the engine's own descriptions; the models here only join them, so that
 * what an agent holds and what it will be refused are on screen together.
 *
 * The rest of the screen's decisions are split by section: the grants in
 * `grantsModel`, the policy in `policyModel`, the create form in
 * `createModel`, memory in `memoryModel`, and the detail page's own layout in
 * `detailModel`.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { ToolView } from "../../bindings/ToolView";
import type { IconName } from "../../components/icons";
import { compareRisk } from "../../components/status";

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/**
 * The agents whose name, provider or model contains `query`, ignoring case.
 *
 * `provider/model` is matched as one string as well, because that is how the
 * card writes it, and a person types what they read.
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

/** Which agents the list shows, by whether they may be given work. */
export type StatusFilter = "all" | "enabled" | "disabled";

/** The status filter's choices, in the order the control offers them. */
export const STATUS_FILTERS: readonly { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "enabled", label: "Enabled" },
  { value: "disabled", label: "Disabled" },
];

/**
 * Whether `agent` belongs under `filter`.
 *
 * Anything but `enabled` counts as disabled, as the status chip draws it: a
 * status this build does not know is not one the operator can give work to.
 */
export function matchesStatus(agent: AgentSummary, filter: StatusFilter): boolean {
  if (filter === "all") return true;
  const enabled = agent.status === "enabled";
  return filter === "enabled" ? enabled : !enabled;
}

// ---------------------------------------------------------------------------
// Risk and tools
// ---------------------------------------------------------------------------

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

/** The figures over the list. */
export interface AgentStats {
  total: number;
  enabled: number;
  disabled: number;
  /**
   * Agents holding at least one high or critical tool, or `null` until the
   * catalogue has answered: before then the screen cannot know, and a zero
   * would be a claim of safety it had not checked.
   */
  highRisk: number | null;
  /** Distinct tool names granted across every agent. */
  toolsInUse: number;
  /** Distinct model providers the agents run on. */
  providers: number;
}

/** Count what the list's figures show. */
export function agentStats(
  agents: readonly AgentSummary[],
  catalogue: readonly ToolView[] | null,
): AgentStats {
  const enabled = agents.filter((agent) => matchesStatus(agent, "enabled")).length;
  const tools = new Set(agents.flatMap((agent) => agent.tools));
  const highRisk =
    catalogue === null
      ? null
      : agents.filter((agent) => {
          const highest = highestRisk(agent.tools, catalogue);
          return highest !== null && compareRisk(highest, "high") >= 0;
        }).length;
  return {
    total: agents.length,
    enabled,
    disabled: agents.length - enabled,
    highRisk,
    toolsInUse: tools.size,
    providers: new Set(agents.map((agent) => agent.provider)).size,
  };
}

/** How many tools, in words. */
export function toolCountLabel(count: number): string {
  if (count === 0) return "No tools";
  return count === 1 ? "1 tool" : `${count} tools`;
}

/**
 * The domains an agent's tools come from, in the order first granted:
 * `browser.navigate` and `browser.extract` are both `browser`.
 *
 * A name with no dot is its own domain, so nothing granted goes unlisted.
 */
export function toolDomains(tools: readonly string[]): string[] {
  const seen: string[] = [];
  for (const name of tools) {
    const at = name.indexOf(".");
    const domain = at === -1 ? name : name.slice(0, at);
    if (domain !== "" && !seen.includes(domain)) seen.push(domain);
  }
  return seen;
}

/**
 * The icon a tool domain is drawn with. A domain this build has no drawing
 * for gets the generic tool mark rather than a guess.
 */
export function domainIcon(domain: string): IconName {
  switch (domain) {
    case "browser":
      return "window";
    case "network":
      return "globe";
    case "filesystem":
      return "folder";
    case "terminal":
      return "terminal";
    case "github":
      return "link";
    case "agent":
      return "agents";
    case "computer":
      return "cpu";
    default:
      return "zap";
  }
}

// ---------------------------------------------------------------------------
// Avatar
// ---------------------------------------------------------------------------

/**
 * The letters an agent's avatar shows: the initials of its first two words,
 * or the first letter of a one-word name.
 *
 * Words are split where a person would see a break (`web-researcher`,
 * `ops_bot`, `sales.eu`). Letters are taken by code point, so a name that
 * opens with an emoji or an accented letter is not cut in half.
 */
export function monogram(name: string): string {
  const words = name.split(/[\s._-]+/).filter((word) => word !== "");
  const initial = (word: string) => Array.from(word)[0] ?? "";
  const letters =
    words.length >= 2 ? `${initial(words[0]!)}${initial(words[1]!)}` : initial(words[0] ?? "");
  return letters === "" ? "?" : letters.toUpperCase();
}

/** How many gradient variants the avatar stylesheet defines. */
export const AVATAR_VARIANTS = 4;

/**
 * Which of the avatar's gradients an agent wears.
 *
 * Derived from the name alone, so an agent keeps the same face across visits,
 * windows and reloads without anything being stored. The gradients only vary
 * the angle and the order of the two accent ends, which are the colours white
 * text is measured on, so no variant is less readable than another.
 */
export function avatarVariant(name: string): number {
  let hash = 0;
  for (const char of name) {
    hash = (hash * 31 + (char.codePointAt(0) ?? 0)) >>> 0;
  }
  return hash % AVATAR_VARIANTS;
}
