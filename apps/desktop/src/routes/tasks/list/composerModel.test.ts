import { describe, expect, it } from "vitest";

import {
  type ComposerKey,
  isApplePlatform,
  submitHint,
  submitIntent,
  submitKeys,
  waitsLabel,
} from "./composerModel";

const press = (extra: Partial<ComposerKey> = {}): ComposerKey => ({
  key: "Enter",
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  isComposing: false,
  ...extra,
});

describe("submitIntent", () => {
  it("leaves a plain Enter to start a new line", () => {
    expect(submitIntent(press(), 0)).toBeNull();
    expect(submitIntent(press({ key: "a", metaKey: true }), 0)).toBeNull();
  });

  it("runs on Command-Enter or Control-Enter", () => {
    expect(submitIntent(press({ metaKey: true }), 0)).toBe("run");
    expect(submitIntent(press({ ctrlKey: true }), 0)).toBe("run");
  });

  it("queues instead when the task waits for others", () => {
    expect(submitIntent(press({ metaKey: true }), 2)).toBe("queue");
  });

  it("never sends while an input method is composing, or with Alt held", () => {
    expect(submitIntent(press({ metaKey: true, isComposing: true }), 0)).toBeNull();
    expect(submitIntent(press({ ctrlKey: true, altKey: true }), 0)).toBeNull();
  });
});

describe("the send chord", () => {
  it("says Command on Apple platforms and Control elsewhere", () => {
    expect(isApplePlatform("MacIntel")).toBe(true);
    expect(isApplePlatform("iPad")).toBe(true);
    expect(isApplePlatform("Win32")).toBe(false);
    expect(isApplePlatform("Linux x86_64")).toBe(false);
    expect(submitKeys(true)).toEqual({ cap: "⌘ Enter", aria: "Meta+Enter" });
    expect(submitKeys(false)).toEqual({ cap: "Ctrl Enter", aria: "Control+Enter" });
  });

  it("says what sending will do", () => {
    expect(submitHint(0, true)).toBe("Press ⌘ Enter to run it now.");
    expect(submitHint(1, false)).toBe(
      "Press Ctrl Enter to queue it behind the tasks it waits for.",
    );
  });

  it("counts the tasks a new one waits for", () => {
    expect(waitsLabel(0)).toBe("Wait for other tasks");
    expect(waitsLabel(1)).toBe("Waits for 1 task");
    expect(waitsLabel(3)).toBe("Waits for 3 tasks");
  });
});
