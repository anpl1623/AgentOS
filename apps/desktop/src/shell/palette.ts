/**
 * What the command palette offers, and in what order.
 *
 * The palette goes places; it never does things. An entry holds a route, not a
 * callback, so no entry can approve, stop a run or disable an agent: every
 * consequential verb passes through the policy engine and needs its screen's
 * context, and a palette that skipped that context would be a second, thinner
 * path to the same action.
 */

import type { AgentSummary } from "../bindings/AgentSummary";
import type { Route } from "../routes/route";
import { NAV, screenRoute } from "./nav";

/** The groups, in the order they are listed. */
export const PALETTE_GROUPS = ["Screens", "Agents"] as const;

export type PaletteGroup = (typeof PALETTE_GROUPS)[number];

/** One destination. */
export interface PaletteEntry {
  /** Unique across the palette. */
  id: string;
  group: PaletteGroup;
  label: string;
  /** Searched along with the label and shown beside it. */
  hint: string;
  /** A neutral marker, such as an agent being disabled. */
  badge: string | null;
  /** Where choosing the entry goes. */
  route: Route;
}

/** How many entries each group shows. */
export const PER_GROUP = 8;

/** Every screen, in sidebar order. */
export function screenEntries(): PaletteEntry[] {
  return NAV.map((item) => ({
    id: `screen:${item.route}`,
    group: "Screens",
    label: item.label,
    hint: item.description,
    badge: null,
    route: screenRoute(item.route),
  }));
}

/**
 * Every agent, in the order the runtime lists them. A disabled agent is still
 * a place to go — that is where it is enabled again — so it is listed with a
 * badge rather than left out.
 */
export function agentEntries(agents: readonly AgentSummary[]): PaletteEntry[] {
  return agents.map((agent) => ({
    id: `agent:${agent.id}`,
    group: "Agents",
    label: agent.name,
    hint: `${agent.provider}/${agent.model}`,
    badge: agent.status === "enabled" ? null : "disabled",
    route: { name: "agents", agent: agent.name },
  }));
}

/**
 * Where `query` first matches `text` as a case-insensitive subsequence, or -1.
 *
 * Whitespace in the query is ignored, so `crm assist` finds `crm-assistant`.
 * An empty query matches everything at 0. The leftmost occurrence of the first
 * character always begins a match when any start does, so it is the index.
 */
export function matchIndex(text: string, query: string): number {
  const needle = query.replace(/\s+/g, "").toLowerCase();
  if (needle === "") return 0;
  const haystack = text.toLowerCase();

  let start = -1;
  let from = 0;
  for (const char of needle) {
    const found = haystack.indexOf(char, from);
    if (found === -1) return -1;
    if (start === -1) start = found;
    from = found + char.length;
  }
  return start;
}

/** How well a query matches an entry, as a tier, best first. */
interface Quality {
  tier: number;
  /** Where the match starts, within the tier's text. */
  at: number;
}

/**
 * How well `query` matches an entry, or `null` if it does not.
 *
 * The label outranks the hint, and a closer match outranks a looser one: the
 * whole label, then its start, then a run of it, then its letters in order,
 * and only then the label and hint together. Without the tiers a short name
 * is beaten by any long description its letters happen to thread through:
 * `ops` is in "Agents, their t*o*ols and *p*olicie*s*" as well as being an
 * agent called ops.
 */
export function matchQuality(entry: PaletteEntry, query: string): Quality | null {
  const needle = query.replace(/\s+/g, "").toLowerCase();
  if (needle === "") return { tier: 0, at: 0 };
  const label = entry.label.toLowerCase();
  if (label === needle) return { tier: 0, at: 0 };
  if (label.startsWith(needle)) return { tier: 1, at: 0 };
  const run = label.indexOf(needle);
  if (run !== -1) return { tier: 2, at: run };
  const scattered = matchIndex(entry.label, query);
  if (scattered !== -1) return { tier: 3, at: scattered };
  const anywhere = matchIndex(`${entry.label} ${entry.hint}`, query);
  return anywhere === -1 ? null : { tier: 4, at: anywhere };
}

/**
 * The entries that match, best first, at most `perGroup` from each group.
 *
 * Entries stay together by group, because the list is drawn under group
 * headings, but the group holding the best match comes first, so the entry
 * Enter takes is the closest match rather than the first group's best. On a
 * tie the groups keep {@link PALETTE_GROUPS} order. Within a group: by
 * quality, then where the match starts, then list order.
 */
export function rankEntries(
  entries: readonly PaletteEntry[],
  query: string,
  perGroup = PER_GROUP,
): PaletteEntry[] {
  const groups = PALETTE_GROUPS.map((group, position) => {
    const matches = entries
      .filter((entry) => entry.group === group)
      .map((entry, order) => ({ entry, order, quality: matchQuality(entry, query) }))
      .flatMap(({ entry, order, quality }) => (quality === null ? [] : [{ entry, order, quality }]))
      .sort(
        (a, b) =>
          a.quality.tier - b.quality.tier || a.quality.at - b.quality.at || a.order - b.order,
      )
      .slice(0, perGroup);
    return { position, best: matches[0]?.quality.tier ?? Infinity, matches };
  });
  groups.sort((a, b) => a.best - b.best || a.position - b.position);
  return groups.flatMap((group) => group.matches.map((match) => match.entry));
}

/** The next active index after moving `by`, wrapping at both ends. */
export function moveActive(current: number, by: number, length: number): number {
  if (length === 0) return 0;
  return (((current + by) % length) + length) % length;
}
