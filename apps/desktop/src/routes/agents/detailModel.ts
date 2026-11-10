/**
 * What the agent detail page decides about its own shape: its sections, which
 * one the reader is in, and the figures its overview leads with.
 */

import type { TaskSummary } from "../../bindings/TaskSummary";
import type { IconName } from "../../components/icons";

/** One section of the detail page, as the in-page navigation lists it. */
export interface DetailSection {
  /** The id of the section's heading, which the navigation scrolls to. */
  id: string;
  label: string;
  icon: IconName;
}

/**
 * The sections, top to bottom. The ids are the headings' ids, which the lists
 * inside each section are also labelled by.
 */
export const DETAIL_SECTIONS: readonly DetailSection[] = [
  { id: "agent-overview", label: "Overview", icon: "info" },
  { id: "agent-tools", label: "Tools & grants", icon: "zap" },
  { id: "agent-policy", label: "Policy", icon: "shield" },
  { id: "agent-memory", label: "Memory", icon: "sparkles" },
  { id: "agent-recent", label: "Recent tasks", icon: "tasks" },
];

/**
 * How far below the top of the window a heading must have scrolled for its
 * section to count as the one being read: past the pinned page header, with a
 * little room so a heading that has just arrived under it counts.
 */
export const READING_LINE = 140;

/**
 * Which section the reader is in.
 *
 * The last section whose heading has risen to the reading line, or the first
 * when none has. At the very end of the page the last section is current even
 * if its heading never reaches the line, because a short final section can be
 * scrolled to the bottom of the window but no further, and a navigation that
 * can never mark it would look broken.
 */
export function activeSection(
  positions: readonly { id: string; top: number }[],
  line: number,
  atEnd: boolean,
): string | null {
  if (positions.length === 0) return null;
  if (atEnd) return positions[positions.length - 1]!.id;
  let current = positions[0]!.id;
  for (const { id, top } of positions) {
    if (top <= line) current = id;
  }
  return current;
}

/** Whether a scrolled box has reached its end, within a pixel of rounding. */
export function scrolledToEnd(
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
): boolean {
  return scrollHeight - (scrollTop + clientHeight) <= 2;
}

/** The figures the overview gives for an agent's recent work. */
export interface TaskTally {
  total: number;
  running: number;
}

/** Count recent tasks, and how many of them are running now. */
export function taskTally(tasks: readonly TaskSummary[]): TaskTally {
  return {
    total: tasks.length,
    running: tasks.filter((task) => task.status === "running").length,
  };
}
