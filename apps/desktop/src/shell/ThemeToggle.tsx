import type { CSSProperties } from "react";

import { Icon } from "../components/icons";
import { tip } from "../components/Tooltip";
import { nextThemePreference, useTheme } from "../sdk/theme";
import { THEME_CHOICES, themeChoice } from "./themeChoices";

/**
 * The theme control at the foot of the sidebar: match the system, dark, or
 * light.
 *
 * In the full sidebar it is three pressed-or-not buttons with a thumb that
 * slides to the chosen one; the pressed state is the announced state, as on
 * every segmented control in the app. The rail has room for one button, so
 * there it is a single button showing the current choice that moves to the
 * next. Which of the two is drawn is the sidebar's container query; the other
 * is `display: none` and so absent for assistive technology too.
 */
export function ThemeToggle() {
  const { preference, setPreference } = useTheme();
  const current = themeChoice(preference);
  const next = themeChoice(nextThemePreference(preference));
  const index = Math.max(
    0,
    THEME_CHOICES.findIndex((choice) => choice.value === current.value),
  );

  return (
    <div className="theme-toggle">
      <div
        className="theme-segments"
        role="group"
        aria-label="Theme"
        style={{ "--index": index, "--count": THEME_CHOICES.length } as CSSProperties}
      >
        <span className="theme-thumb" aria-hidden="true" />
        {THEME_CHOICES.map((choice) => (
          <button
            key={choice.value}
            type="button"
            aria-pressed={choice.value === current.value}
            onClick={() => setPreference(choice.value)}
            {...tip(choice.label, "top")}
          >
            <Icon name={choice.icon} size={16} />
          </button>
        ))}
      </div>
      <button
        type="button"
        className="icon-button theme-cycle"
        onClick={() => setPreference(next.value)}
        {...tip(`Theme: ${current.label}. Switch to ${next.label.toLowerCase()}`, "right")}
      >
        <Icon name={current.icon} />
      </button>
    </div>
  );
}
