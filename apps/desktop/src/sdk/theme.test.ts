import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  THEME_CROSSFADE_MS,
  THEME_PREFERENCES,
  THEME_STORAGE_KEY,
  THEME_SWITCHING_CLASS,
  isThemePreference,
  nextThemePreference,
  parseThemePreference,
  readThemePreference,
  resolveTheme,
} from "./theme";

describe("resolveTheme", () => {
  it("follows the system when asked to", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("ignores the system when a theme was chosen", () => {
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
  });
});

describe("parseThemePreference", () => {
  it.each(THEME_PREFERENCES)("reads %s back as itself", (preference) => {
    expect(parseThemePreference(preference)).toBe(preference);
  });

  it.each([null, undefined, "", "Dark", "sepia", "constructor"])(
    "reads %j as system",
    (raw) => {
      expect(parseThemePreference(raw)).toBe("system");
    },
  );

  it("agrees with isThemePreference", () => {
    expect(isThemePreference("light")).toBe(true);
    expect(isThemePreference(1)).toBe(false);
    expect(isThemePreference("toString")).toBe(false);
  });
});

describe("nextThemePreference", () => {
  it("cycles through every preference and comes back", () => {
    expect(nextThemePreference("system")).toBe("dark");
    expect(nextThemePreference("dark")).toBe("light");
    expect(nextThemePreference("light")).toBe("system");
  });
});

/** A storage that holds values, or one whose every call throws. */
function storage(initial: Record<string, string> = {}, broken = false) {
  const values = new Map(Object.entries(initial));
  const fail = () => {
    throw new Error("storage is disabled");
  };
  return {
    values,
    getItem: (key: string) => (broken ? fail() : (values.get(key) ?? null)),
    setItem: (key: string, value: string) => (broken ? fail() : void values.set(key, value)),
  };
}

describe("readThemePreference", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reads the stored preference", () => {
    vi.stubGlobal("localStorage", storage({ [THEME_STORAGE_KEY]: "light" }));
    expect(readThemePreference()).toBe("light");
  });

  it("falls back to system when nothing is stored", () => {
    vi.stubGlobal("localStorage", storage());
    expect(readThemePreference()).toBe("system");
  });

  it("falls back to system when storage throws", () => {
    vi.stubGlobal("localStorage", storage({}, true));
    expect(readThemePreference()).toBe("system");
  });

  it("falls back to system when there is no storage at all", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(readThemePreference()).toBe("system");
  });
});

/**
 * The document as far as the module touches it: <html>'s dataset, its style's
 * colour scheme, and its class list. Enough to check what is written without a
 * simulated browser.
 */
function fakeDocument() {
  const classes = new Set<string>();
  const root = {
    dataset: {} as Record<string, string | undefined>,
    style: { colorScheme: "" },
    classList: {
      add: (name: string) => void classes.add(name),
      remove: (name: string) => void classes.delete(name),
      contains: (name: string) => classes.has(name),
    },
  };
  return { documentElement: root };
}

/** matchMedia answering the two questions the module asks, with a way to flip one. */
function fakeMedia(options: { dark: boolean; reducedMotion?: boolean }) {
  const listeners: (() => void)[] = [];
  const media = (query: string) => ({
    get matches() {
      return query.includes("color-scheme") ? options.dark : (options.reducedMotion ?? false);
    },
    addEventListener: (_: string, listener: () => void) => void listeners.push(listener),
  });
  return {
    media,
    flip(dark: boolean) {
      options.dark = dark;
      for (const listener of listeners) listener();
    },
  };
}

describe("the theme on the document", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function load(options: {
    stored?: string;
    dark: boolean;
    reducedMotion?: boolean;
  }) {
    const document = fakeDocument();
    const media = fakeMedia(options);
    const store = storage(options.stored ? { [THEME_STORAGE_KEY]: options.stored } : {});
    const handlers: ((event: { key: string | null }) => void)[] = [];
    vi.stubGlobal("document", document);
    vi.stubGlobal("matchMedia", media.media);
    vi.stubGlobal("localStorage", store);
    vi.stubGlobal(
      "addEventListener",
      (type: string, handler: (event: { key: string | null }) => void) => {
        if (type === "storage") handlers.push(handler);
      },
    );
    const theme = await import("./theme");
    return {
      theme,
      root: document.documentElement,
      store,
      media,
      storageEvent: (key: string | null) => {
        for (const handler of handlers) handler({ key });
      },
    };
  }

  it("writes the resolved theme and colour scheme before the first render", async () => {
    const { theme, root } = await load({ dark: false });
    theme.applyTheme();
    expect(root.dataset.theme).toBe("light");
    expect(root.style.colorScheme).toBe("light");
    expect(theme.currentTheme()).toEqual({ preference: "system", resolved: "light" });
  });

  it("does not cross-fade on load", async () => {
    const { theme, root } = await load({ stored: "dark", dark: false });
    theme.applyTheme();
    expect(root.dataset.theme).toBe("dark");
    expect(root.classList.contains(THEME_SWITCHING_CLASS)).toBe(false);
  });

  it("cross-fades a change of theme, and only while it runs", async () => {
    const { theme, root, store } = await load({ dark: true });
    theme.applyTheme();
    theme.setThemePreference("light");
    expect(root.dataset.theme).toBe("light");
    expect(store.values.get(THEME_STORAGE_KEY)).toBe("light");
    expect(root.classList.contains(THEME_SWITCHING_CLASS)).toBe(true);
    vi.advanceTimersByTime(THEME_CROSSFADE_MS + 100);
    expect(root.classList.contains(THEME_SWITCHING_CLASS)).toBe(false);
  });

  it("does not cross-fade under reduced motion", async () => {
    const { theme, root } = await load({ dark: true, reducedMotion: true });
    theme.applyTheme();
    theme.setThemePreference("light");
    expect(root.dataset.theme).toBe("light");
    expect(root.classList.contains(THEME_SWITCHING_CLASS)).toBe(false);
  });

  it("does not cross-fade a choice that changes nothing on screen", async () => {
    const { theme, root } = await load({ dark: true });
    theme.applyTheme();
    theme.setThemePreference("dark");
    expect(theme.currentTheme()).toEqual({ preference: "dark", resolved: "dark" });
    expect(root.classList.contains(THEME_SWITCHING_CLASS)).toBe(false);
  });

  it("follows the system while the preference is system", async () => {
    const { theme, root, media } = await load({ dark: true });
    theme.applyTheme();
    media.flip(false);
    expect(root.dataset.theme).toBe("light");
    expect(theme.currentTheme().resolved).toBe("light");
  });

  it("ignores the system once a theme was chosen", async () => {
    const { theme, root, media } = await load({ stored: "dark", dark: true });
    theme.applyTheme();
    media.flip(false);
    expect(root.dataset.theme).toBe("dark");
  });

  it("follows a choice made in another window", async () => {
    const { theme, root, store, storageEvent } = await load({ dark: true });
    theme.applyTheme();
    store.values.set(THEME_STORAGE_KEY, "light");
    storageEvent(THEME_STORAGE_KEY);
    expect(theme.currentTheme()).toEqual({ preference: "light", resolved: "light" });
    expect(root.dataset.theme).toBe("light");
  });

  // useSyncExternalStore re-renders whenever the snapshot is a new object, and
  // loops if it is new on every read, so it must change exactly when the theme
  // or the preference does.
  it("keeps the same snapshot until something changes", async () => {
    const { theme, storageEvent } = await load({ dark: true });
    theme.applyTheme();
    const before = theme.currentTheme();
    expect(theme.currentTheme()).toBe(before);
    storageEvent("some.other.key");
    theme.applyTheme();
    expect(theme.currentTheme()).toBe(before);
    theme.setThemePreference("light");
    expect(theme.currentTheme()).not.toBe(before);
  });

  it("refuses a value that is not a preference", async () => {
    const { theme, store } = await load({ dark: true });
    theme.applyTheme();
    theme.setThemePreference("sepia" as never);
    expect(store.values.has(THEME_STORAGE_KEY)).toBe(false);
    expect(theme.currentTheme().preference).toBe("system");
  });
});
