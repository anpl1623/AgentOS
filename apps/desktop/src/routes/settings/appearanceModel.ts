/**
 * The appearance cards: one per theme preference, in the order the theme
 * module offers them, each with a preview of what it shows.
 *
 * Storing, resolving and applying a preference are `sdk/theme.ts`'s; this is
 * only how the settings screen offers the choice.
 */

import type { IconName } from "../../components/icons";
import { THEME_PREFERENCES, type ResolvedTheme, type ThemePreference } from "../../sdk/theme";

/** One card. */
export interface AppearanceChoice {
  value: ThemePreference;
  /** The choice's name, the same words the sidebar's theme control uses. */
  label: string;
  icon: IconName;
  /** A sentence under the name: what choosing it means. */
  note: string;
  /**
   * What the preview draws: one theme, or both side by side for the choice
   * that follows the system, dark first as the home theme.
   */
  preview: readonly ResolvedTheme[];
}

const FACES: Record<ThemePreference, Pick<AppearanceChoice, "label" | "icon" | "preview">> = {
  system: { label: "Match the system", icon: "monitor", preview: ["dark", "light"] },
  dark: { label: "Dark", icon: "moon", preview: ["dark"] },
  light: { label: "Light", icon: "sun", preview: ["light"] },
};

/**
 * Every card, in the theme module's order.
 *
 * The system card says what the system resolves to only while it is the
 * choice: that is the one time `resolved` is the system's answer rather than
 * the operator's, and naming it then shows the choice working.
 */
export function appearanceChoices(
  preference: ThemePreference,
  resolved: ResolvedTheme,
): AppearanceChoice[] {
  return THEME_PREFERENCES.map((value) => ({
    value,
    ...FACES[value],
    note: noteFor(value, preference, resolved),
  }));
}

function noteFor(
  value: ThemePreference,
  preference: ThemePreference,
  resolved: ResolvedTheme,
): string {
  switch (value) {
    case "system":
      return preference === "system"
        ? `Follows this computer, which is ${resolved} right now.`
        : "Follows this computer's light or dark setting, and changes when it does.";
    case "dark":
      return "Deep ink, with light kept for what needs you.";
    case "light":
      return "Paper white and cool greys, for a bright room.";
  }
}

/**
 * Whether a preview of `shows` has to be drawn inverted.
 *
 * The previews are drawn in the theme's own tokens, which hold only the theme
 * on screen; a preview of the other one is the same drawing turned over, so
 * the screen needs no second set of colours to show it.
 */
export function previewInverted(shows: ResolvedTheme, resolved: ResolvedTheme): boolean {
  return shows !== resolved;
}
