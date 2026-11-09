import { describe, expect, it } from "vitest";

import { isIconName } from "../../components/icons";
import { THEME_PREFERENCES } from "../../sdk/theme";
import { appearanceChoices, previewInverted } from "./appearanceModel";

describe("appearanceChoices", () => {
  it("offers every preference once, in the theme module's order", () => {
    const choices = appearanceChoices("system", "dark");
    expect(choices.map((choice) => choice.value)).toEqual([...THEME_PREFERENCES]);
    for (const choice of choices) expect(isIconName(choice.icon)).toBe(true);
  });

  it("previews both themes for the system choice and one for each of the others", () => {
    const [system, dark, light] = appearanceChoices("dark", "dark");
    expect(system!.preview).toEqual(["dark", "light"]);
    expect(dark!.preview).toEqual(["dark"]);
    expect(light!.preview).toEqual(["light"]);
  });

  it("says what the system resolves to only while it is the choice", () => {
    expect(appearanceChoices("system", "light")[0]!.note).toBe(
      "Follows this computer, which is light right now.",
    );
    // With dark chosen by hand, `resolved` is the operator's answer, not the
    // system's, and naming it on the system card would be a guess.
    const chosen = appearanceChoices("dark", "dark")[0]!.note;
    expect(chosen).not.toMatch(/right now/);
    expect(chosen).toMatch(/changes when it does/);
  });
});

describe("previewInverted", () => {
  it("turns over only the preview of the theme that is not on screen", () => {
    expect(previewInverted("dark", "dark")).toBe(false);
    expect(previewInverted("light", "dark")).toBe(true);
    expect(previewInverted("dark", "light")).toBe(true);
    expect(previewInverted("light", "light")).toBe(false);
  });
});
