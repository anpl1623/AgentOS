import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FOCUS_DEBOUNCE_MS, type VisibilityHost, startVisibleInterval } from "./live";

/** A window whose visibility and focus the test controls. */
function fakeHost(visible: boolean) {
  let isVisible = visible;
  const visibility = new Set<() => void>();
  const focus = new Set<() => void>();
  const host: VisibilityHost = {
    isVisible: () => isVisible,
    onVisibilityChange: (callback) => {
      visibility.add(callback);
      return () => visibility.delete(callback);
    },
    onFocus: (callback) => {
      focus.add(callback);
      return () => focus.delete(callback);
    },
    now: () => Date.now(),
  };
  return {
    host,
    show() {
      isVisible = true;
      for (const each of visibility) each();
    },
    hide() {
      isVisible = false;
      for (const each of visibility) each();
    },
    focus() {
      for (const each of focus) each();
    },
    listeners: () => visibility.size + focus.size,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("startVisibleInterval", () => {
  it("ticks while visible, without firing on start", () => {
    const fn = vi.fn();
    const window = fakeHost(true);
    startVisibleInterval(fn, 1000, window.host);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(3000);
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it("does nothing while hidden", () => {
    const fn = vi.fn();
    const window = fakeHost(false);
    startVisibleInterval(fn, 1000, window.host);
    vi.advanceTimersByTime(10_000);
    expect(fn).not.toHaveBeenCalled();
  });

  it("stops when hidden and fires at once on becoming visible", () => {
    const fn = vi.fn();
    const window = fakeHost(true);
    startVisibleInterval(fn, 1000, window.host);
    window.hide();
    vi.advanceTimersByTime(5000);
    expect(fn).not.toHaveBeenCalled();
    window.show();
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1000);
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it("does not fetch twice when a window returns with both events", () => {
    const fn = vi.fn();
    const window = fakeHost(false);
    startVisibleInterval(fn, 60_000, window.host);
    window.show();
    window.focus();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("fires on focus once the debounce has passed", () => {
    const fn = vi.fn();
    const window = fakeHost(false);
    startVisibleInterval(fn, 60_000, window.host);
    window.show();
    vi.advanceTimersByTime(FOCUS_DEBOUNCE_MS);
    window.focus();
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it("releases its timer and listeners when stopped", () => {
    const fn = vi.fn();
    const window = fakeHost(true);
    const stop = startVisibleInterval(fn, 1000, window.host);
    stop();
    vi.advanceTimersByTime(5000);
    expect(fn).not.toHaveBeenCalled();
    expect(window.listeners()).toBe(0);
  });
});
