/**
 * How the theme control names and draws each preference.
 *
 * The preferences themselves — their order, the cycle through them, storing
 * and resolving them — are `sdk/theme.ts`'s. This is only how the sidebar
 * offers them, in that module's order.
 */

import type { IconName } from "../components/icons";
import { THEME_PREFERENCES, type ThemePreference } from "../sdk/theme";

export interface ThemeChoice {
  value: ThemePreference;
  /** The choice's name, read aloud and shown as its tooltip. */
  label: string;
  icon: IconName;
}

const FACES: Record<ThemePreference, Omit<ThemeChoice, "value">> = {
  system: { label: "Match the system", icon: "monitor" },
  dark: { label: "Dark", icon: "moon" },
  light: { label: "Light", icon: "sun" },
};

/** Every choice, in the order the theme module offers them. */
export const THEME_CHOICES: readonly ThemeChoice[] = THEME_PREFERENCES.map((value) => ({
  value,
  ...FACES[value],
}));

/** The choice for a preference. */
export function themeChoice(preference: ThemePreference): ThemeChoice {
  return { value: preference, ...FACES[preference] };
}
