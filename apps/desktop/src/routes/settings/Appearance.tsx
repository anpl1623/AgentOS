import { useId, useState } from "react";

import { Icon } from "../../components/common";
import { type ResolvedTheme, useTheme } from "../../sdk/theme";
import { appearanceChoices, previewInverted } from "./appearanceModel";
import { Section } from "./parts";

/**
 * How the window looks: match the system, dark or light, as three cards that
 * each show a small picture of the theme they choose.
 *
 * A choice is a setting rather than a form, so it takes effect as it is made,
 * through the same store the sidebar's theme control uses; the two stay in
 * step without either knowing of the other. The cards are native radio
 * buttons inside their labels, so the arrow keys move between them and the
 * group is announced as one choice of three.
 */
export function Appearance() {
  const { preference, resolved, setPreference } = useTheme();
  const name = useId();
  const choices = appearanceChoices(preference, resolved);
  // The check pops when a choice is made here, not when the screen opens on
  // the choice already made: a pop says something changed.
  const [chose, setChose] = useState(false);

  return (
    <Section
      id="settings-appearance"
      intro="How AgentOS looks on this computer. A choice takes effect at once."
    >
      <fieldset className="settings-themes" data-chose={chose ? "" : undefined}>
        <legend className="visually-hidden">Theme</legend>
        {choices.map((choice) => {
          const chosen = choice.value === preference;
          const labelId = `${name}-${choice.value}`;
          return (
            <label
              key={choice.value}
              className={chosen ? "settings-theme lift chosen" : "settings-theme lift"}
              data-spotlight
            >
              <input
                type="radio"
                className="visually-hidden"
                name={name}
                value={choice.value}
                checked={chosen}
                aria-labelledby={labelId}
                aria-describedby={`${labelId}-note`}
                onChange={() => {
                  setChose(true);
                  setPreference(choice.value);
                }}
              />
              <ThemePreview shows={choice.preview} resolved={resolved} />
              <span className="settings-theme-name">
                <Icon name={choice.icon} size={16} />
                <span id={labelId}>{choice.label}</span>
                <span className="settings-theme-check" aria-hidden="true">
                  <Icon name="check" size={12} />
                </span>
              </span>
              <span className="settings-theme-note" id={`${labelId}-note`}>
                {choice.note}
              </span>
            </label>
          );
        })}
      </fieldset>
    </Section>
  );
}

/**
 * A window in miniature: the sidebar with its current item, a title, and two
 * cards with a primary button. Drawn in the tokens of the theme on screen; a
 * theme that is not on screen is the same drawing inverted, so no colour
 * outside `tokens.css` is needed to show it. With two themes the second takes
 * the lower right half, split on the diagonal.
 */
function ThemePreview({
  shows,
  resolved,
}: {
  shows: readonly ResolvedTheme[];
  resolved: ResolvedTheme;
}) {
  return (
    <span className="settings-preview" aria-hidden="true">
      {shows.map((theme, index) => (
        <span
          key={theme}
          className="settings-mini"
          data-inverted={previewInverted(theme, resolved) ? "" : undefined}
          data-half={shows.length > 1 && index > 0 ? "" : undefined}
        >
          <span className="settings-mini-side">
            <span className="settings-mini-brand" />
            <span className="settings-mini-nav current" />
            <span className="settings-mini-nav" />
            <span className="settings-mini-nav" />
          </span>
          <span className="settings-mini-main">
            <span className="settings-mini-title" />
            <span className="settings-mini-card">
              <span className="settings-mini-line" />
              <span className="settings-mini-line short" />
            </span>
            <span className="settings-mini-card">
              <span className="settings-mini-line short" />
              <span className="settings-mini-button" />
            </span>
          </span>
        </span>
      ))}
    </span>
  );
}
