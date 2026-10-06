import { beforeEach, describe, expect, it } from "vitest";

import {
  CACHE_ENTRIES,
  CACHE_FRESH_MS,
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
