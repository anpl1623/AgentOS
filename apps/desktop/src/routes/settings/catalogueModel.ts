/**
 * The tool catalogue: every tool, by domain, with who on this machine holds
 * it, and which tools a filter keeps.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { ToolView } from "../../bindings/ToolView";
import { compareRisk } from "../../components/status";

/** An agent that has been granted a tool. */
export interface Holder {
  id: string;
  name: string;
  /** A disabled agent keeps its grants; enabling it gives them back. */
  enabled: boolean;
}

/** One tool, with who on this machine holds it. */
export interface CatalogueEntry {
  tool: ToolView;
  /**
   * The agents granted it, by name. `null` when the agents could not be read:
   * "no agent" would then be a claim nobody checked.
   */
  holders: Holder[] | null;
}

/** The tools of one domain, most dangerous first. */
export interface CatalogueGroup {
  domain: string;
  entries: CatalogueEntry[];
}

/** The domain a tool belongs to, from its view or else from its name. */
function domainOf(tool: ToolView): string {
  if (tool.domain !== "") return tool.domain;
  const dot = tool.name.indexOf(".");
  return dot === -1 ? tool.name : tool.name.slice(0, dot);
}

/**
 * The agents granted `tool`, ordered by name.
 *
 * Disabled agents are included. A grant is a standing fact about the agent,
 * not about whether it is running today, and a disabled agent holding
 * `terminal.exec` is one switch away from using it.
 */
export function holdersOf(tool: string, agents: readonly AgentSummary[]): Holder[] {
  return agents
    .filter((agent) => agent.tools.includes(tool))
    .map((agent) => ({ id: agent.id, name: agent.name, enabled: agent.status === "enabled" }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/**
 * The filter as lower-case terms. Every term must match somewhere in an entry,
 * so `filesystem high` narrows rather than widens.
 */
export function filterTerms(filter: string): string[] {
  return filter.toLowerCase().split(/\s+/).filter((term) => term !== "");
}

/**
 * Whether an entry is kept by the filter.
 *
 * A term may match the tool's name, domain, description, risk, a declared
 * capability, or the name of an agent that holds it, so both "who can run
 * `terminal.exec`?" and "what does `sales` hold?" are one word in the box.
 */
export function entryMatches(entry: CatalogueEntry, terms: readonly string[]): boolean {
  if (terms.length === 0) return true;
  const { tool } = entry;
  const haystack = [
    tool.name,
    tool.domain,
    tool.description,
    tool.risk,
    ...tool.capabilities,
    ...(tool.returns_untrusted_data ? ["external"] : []),
    ...(entry.holders ?? []).map((holder) => holder.name),
  ]
    .join("\n")
    .toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

/**
 * The catalogue grouped by domain, filtered, with each tool's holders.
 *
 * Domains are alphabetical. Within one, the most dangerous tool comes first,
 * because the question this screen answers is who holds the dangerous ones;
 * ties go by name. A domain the filter empties is dropped rather than shown
 * as a heading over nothing.
 */
export function catalogue(
  tools: readonly ToolView[],
  agents: readonly AgentSummary[] | null,
  filter: string,
): CatalogueGroup[] {
  const terms = filterTerms(filter);
  const groups = new Map<string, CatalogueEntry[]>();
  for (const tool of tools) {
    const entry: CatalogueEntry = {
      tool,
      holders: agents === null ? null : holdersOf(tool.name, agents),
    };
    if (!entryMatches(entry, terms)) continue;
    const domain = domainOf(tool);
    const entries = groups.get(domain);
    if (entries) entries.push(entry);
    else groups.set(domain, [entry]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([domain, entries]) => ({
      domain,
      entries: entries.sort(
        (a, b) => compareRisk(b.tool.risk, a.tool.risk) || a.tool.name.localeCompare(b.tool.name),
      ),
    }));
}

/** How many tools a grouped catalogue holds. */
export function countEntries(groups: readonly CatalogueGroup[]): number {
  return groups.reduce((total, group) => total + group.entries.length, 0);
}

/** The holders line of a catalogue row, and whether it names nobody. */
export interface GrantLine {
  text: string;
  /** Nobody holds it, which is drawn quieter than a list of names. */
  nobody: boolean;
}

/**
 * Who holds a tool, as the row says it.
 *
 * A disabled holder is named as one, so a grant nobody is using today is not
 * mistaken for one that is live, nor hidden as if it were gone.
 */
export function grantLine(holders: readonly Holder[] | null): GrantLine {
  if (holders === null) return { text: "who holds it could not be read", nobody: true };
  if (holders.length === 0) return { text: "granted to no agent", nobody: true };
  const names = holders.map((holder) =>
    holder.enabled ? holder.name : `${holder.name} (disabled)`,
  );
  return { text: `granted to ${names.join(", ")}`, nobody: false };
}

/** The catalogue in three figures, for the line above the filter. */
export interface CatalogueSummary {
  tools: number;
  /** Tools at high risk or above, an unknown level included. */
  dangerous: number;
  /** Tools whose output someone other than the operator can write. */
  external: number;
}

/**
 * How many tools there are, how many are dangerous, and how many read
 * untrusted data. A risk this build does not know counts as dangerous, as it
 * sorts above critical in the list: a level nobody here has heard of is not
 * one to assume is mild.
 */
export function catalogueSummary(tools: readonly ToolView[]): CatalogueSummary {
  return {
    tools: tools.length,
    dangerous: tools.filter((tool) => compareRisk(tool.risk, "high") >= 0).length,
    external: tools.filter((tool) => tool.returns_untrusted_data).length,
  };
}
