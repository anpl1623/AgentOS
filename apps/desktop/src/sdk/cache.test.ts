import { beforeEach, describe, expect, it } from "vitest";

import {
  CACHE_ENTRIES,
  CACHE_FRESH_MS,
  type Scrollable,
  applyScroll,
  cacheKey,
  clearCache,
  readCache,
  writeCache,
} from "./cache";

beforeEach(() => {
  clearCache();
});

describe("the answer cache", () => {
  it("keys by command and arguments, so one agent never seeds another", () => {
    writeCache(cacheKey("get_agent", "sales"), "sales detail", 0);
    expect(readCache(cacheKey("get_agent", "support"), 1)).toBeNull();
    expect(readCache(cacheKey("get_agent", "sales"), 1)).toBe("sales detail");
  });

  it("seeds only from answers younger than the freshness limit", () => {
    writeCache("k", 1, 0);
    expect(readCache("k", CACHE_FRESH_MS)).toBe(1);
    expect(readCache("k", CACHE_FRESH_MS + 1)).toBeNull();
  });

  it("evicts the least recently written answer beyond the limit", () => {
    for (let n = 0; n <= CACHE_ENTRIES; n += 1) writeCache(`k${n}`, n, 0);
    expect(readCache("k0", 0)).toBeNull();
    expect(readCache(`k${CACHE_ENTRIES}`, 0)).toBe(CACHE_ENTRIES);
  });
});

/** A scrolling element whose content can grow, clamping as a browser does. */
class Container implements Scrollable {
  private top = 0;
  constructor(public maxScroll: number) {}
  get scrollTop(): number {
    return this.top;
  }
  set scrollTop(value: number) {
    this.top = Math.max(0, Math.min(value, this.maxScroll));
  }
}

describe("restoring a scroll position", () => {
  it("does not count a clamped restore as done, and lands once the content is there", () => {
    // A screen returned to starts from its loading state, too short to hold
    // the position it was left at.
    const main = new Container(0);
    expect(applyScroll(main, 600)).toBe(false);
    expect(main.scrollTop).toBe(0);

    main.maxScroll = 300;
    expect(applyScroll(main, 600)).toBe(false);

    main.maxScroll = 1_377;
    expect(applyScroll(main, 600)).toBe(true);
    expect(main.scrollTop).toBe(600);
  });

  it("takes the top of a screen at once", () => {
    expect(applyScroll(new Container(0), 0)).toBe(true);
  });
});
