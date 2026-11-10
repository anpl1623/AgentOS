import { Suspense, createElement, use } from "react";
import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { type ModuleCache, createModuleCache, track } from "./moduleCache";

/** Let every settled promise's handlers run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

type Modules = { screen: { label: string } };

describe("track", () => {
  it("marks a promise pending, then fulfilled with its value", async () => {
    const promise = track(Promise.resolve(7));
    expect(promise.status).toBe("pending");
    await settle();
    expect(promise).toMatchObject({ status: "fulfilled", value: 7 });
  });

  it("marks a rejection with its reason", async () => {
    const failure = new Error("offline");
    const promise = track(Promise.reject(failure));
    await settle();
    expect(promise).toMatchObject({ status: "rejected", reason: failure });
  });
});

describe("createModuleCache", () => {
  it("hands a preloaded module to the first read already settled", async () => {
    const load = vi.fn(async () => ({ label: "drawn" }));
    const cache = createModuleCache<Modules>({ screen: load });
    await cache.preload("screen");
    // Settled before anything reads it: `use` returns it in the same render.
    expect(cache.read("screen")).toMatchObject({ status: "fulfilled", value: { label: "drawn" } });
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("keeps one promise per module across reads", () => {
    const load = vi.fn(async () => ({ label: "drawn" }));
    const cache = createModuleCache<Modules>({ screen: load });
    expect(cache.read("screen")).toBe(cache.read("screen"));
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("does not fetch again for a preload once a read has asked", async () => {
    const load = vi.fn(async () => ({ label: "drawn" }));
    const cache = createModuleCache<Modules>({ screen: load });
    cache.read("screen");
    await cache.preload("screen");
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("keeps the promise a read is waiting on when a preload lands first", async () => {
    let finish: (module: { label: string }) => void = () => undefined;
    const slow = new Promise<{ label: string }>((resolve) => {
      finish = resolve;
    });
    const load = vi.fn().mockReturnValueOnce(Promise.resolve({ label: "preloaded" })).mockReturnValueOnce(slow);
    const cache = createModuleCache<Modules>({ screen: load });
    const preloading = cache.preload("screen");
    const waited = cache.read("screen");
    await preloading;
    // React is waiting on `waited`; swapping it would leave that wait unanswered.
    expect(cache.read("screen")).toBe(waited);
    finish({ label: "read" });
    await settle();
    expect(waited).toMatchObject({ status: "fulfilled", value: { label: "read" } });
  });

  it("forgets a failed preload, so the first draw fetches again", async () => {
    const load = vi
      .fn<() => Promise<{ label: string }>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ label: "drawn" });
    const cache = createModuleCache<Modules>({ screen: load });
    await cache.preload("screen");
    const read = cache.read("screen");
    await settle();
    expect(read).toMatchObject({ status: "fulfilled", value: { label: "drawn" } });
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("keeps a failed read failed, so a waiting draw meets the error and does not fetch in a loop", async () => {
    const failure = new Error("offline");
    const load = vi.fn<() => Promise<{ label: string }>>().mockRejectedValue(failure);
    const cache = createModuleCache<Modules>({ screen: load });
    const first = cache.read("screen");
    await settle();
    expect(cache.read("screen")).toBe(first);
    expect(first).toMatchObject({ status: "rejected", reason: failure });
    expect(load).toHaveBeenCalledTimes(1);
  });
});

describe("a screen read with use()", () => {
  function Screen({ cache }: { cache: ModuleCache<Modules> }) {
    return use(cache.read("screen")).label;
  }

  // Server rendering cannot wait for anything, so its output is what a render
  // commits on the spot: the screen if it could draw, the fallback if it
  // suspended.
  const draw = (cache: ModuleCache<Modules>) =>
    renderToString(createElement(Suspense, { fallback: "waiting" }, createElement(Screen, { cache })));

  it("draws a preloaded module in the render that first asks for it", async () => {
    const cache = createModuleCache<Modules>({ screen: async () => ({ label: "drawn" }) });
    await cache.preload("screen");
    expect(draw(cache)).toContain("drawn");
  });

  it("waits for a module that has not arrived", () => {
    const cache = createModuleCache<Modules>({ screen: () => new Promise(() => undefined) });
    expect(draw(cache)).toContain("waiting");
  });
});
