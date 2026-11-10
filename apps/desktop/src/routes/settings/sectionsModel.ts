/**
 * The sections of the settings screen, in the order they are drawn, and the
 * decisions behind the in-page navigation: which section is being read, and
 * how far the chip strip scrolls to keep the current chip in view.
 */

import type { IconName } from "../../components/icons";

/** One section: the id of its heading, its name in the navigation, and its icon. */
export interface SectionEntry {
  /** The heading's id, which also names the lists beneath it. */
  id: string;
  label: string;
  icon: IconName;
  /**
   * Drawn only once the settings themselves have been read. The sections that
   * load on their own (integrations, credentials, the scheduler, the audit
   * log) and appearance, which needs nothing, are drawn either way.
   */
  needsSettings: boolean;
}

/**
 * Ordered by what an operator comes here to change. How the window looks
 * first, because it is the one setting everyone changes; then secrets: the
 * model providers', the integration accounts agents act as, and the ones bound
 * to a network origin; then the browser and the scheduler, which decide what
 * can run at all; then the evidence; then the catalogue of what agents can be
 * granted, and where all of it is kept.
 */
export const SECTIONS: readonly SectionEntry[] = [
  { id: "settings-appearance", label: "Appearance", icon: "sun", needsSettings: false },
  { id: "settings-providers", label: "Model providers", icon: "sparkles", needsSettings: true },
  { id: "settings-integrations", label: "Integrations", icon: "link", needsSettings: false },
  { id: "settings-credentials", label: "Network credentials", icon: "key", needsSettings: false },
  { id: "settings-browser", label: "Browser", icon: "globe", needsSettings: true },
  { id: "settings-scheduler", label: "Scheduler", icon: "clock", needsSettings: false },
  { id: "settings-audit", label: "Audit log", icon: "shield", needsSettings: false },
  { id: "settings-tools", label: "Tools", icon: "zap", needsSettings: true },
  { id: "settings-storage", label: "Storage", icon: "folder", needsSettings: true },
];

/** The entry for a section id. Every id the screen uses is in {@link SECTIONS}. */
export function section(id: string): SectionEntry {
  const found = SECTIONS.find((entry) => entry.id === id);
  if (found === undefined) throw new Error(`no settings section is called ${id}`);
  return found;
}

/**
 * The sections actually on the page. While the settings cannot be read, the
 * sections drawn from them are absent, and a link to a heading that is not
 * there would go nowhere.
 */
export function visibleSections(hasSettings: boolean): SectionEntry[] {
  return SECTIONS.filter((entry) => hasSettings || !entry.needsSettings);
}

/** Where a section's heading sits, in the same coordinates as the reading line. */
export interface SectionTop {
  id: string;
  top: number;
}

/**
 * The section being read: the last one, in page order, whose heading has
 * reached the reading line.
 *
 * The line sits just below where a jump from the navigation puts a heading,
 * so the section jumped to is the one marked, however short it is. Above the
 * first heading the first section is marked rather than none. At the foot of
 * the page the last section is marked even if its heading never reaches the
 * line: a short section at the end cannot scroll that far, and would
 * otherwise never be marked at all.
 */
export function activeSection(
  tops: readonly SectionTop[],
  line: number,
  atEnd: boolean,
): string | null {
  const first = tops[0];
  if (first === undefined) return null;
  if (atEnd) return tops[tops.length - 1]!.id;
  let active = first.id;
  for (const { id, top } of tops) {
    if (top <= line) active = id;
  }
  return active;
}

/**
 * Where the chip strip should scroll so the chip at `left` (and `width` wide)
 * is wholly in view with `pad` to spare. It moves only as far as it must, and
 * not at all when the chip is already in view, so marking a section the
 * reader can already see does not shift the strip under their eye.
 */
export function stripScroll(
  left: number,
  width: number,
  scrollLeft: number,
  viewWidth: number,
  pad: number,
): number {
  if (left - pad < scrollLeft) return Math.max(0, left - pad);
  if (left + width + pad > scrollLeft + viewWidth) return left + width + pad - viewWidth;
  return scrollLeft;
}
