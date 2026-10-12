import { describe, expect, it } from "vitest";

import { type AsyncState, initialAsync, stepAsync, viewAsync } from "./useAsync";

function loaded<T>(value: T, generation = 1): AsyncState<T> {
  let state = stepAsync(initialAsync<T>(null), { kind: "start", generation });
  state = stepAsync(state, { kind: "success", generation, value });
  return state;
}

describe("the first load", () => {
  it("is loading, not refreshing, while there is nothing to show", () => {
    const state = stepAsync(initialAsync<string>(null), { kind: "start", generation: 1 });
    expect(viewAsync(state)).toMatchObject({ loading: true, refreshing: false, stale: false });
  });

  it("shows a seed as data being refreshed rather than as a spinner", () => {
    const state = stepAsync(initialAsync("cached"), { kind: "start", generation: 1 });
    expect(viewAsync(state)).toMatchObject({ data: "cached", loading: false, refreshing: true });
  });
});

describe("a reload", () => {
  it("sets refreshing and never loading while data is on screen", () => {
    // A 4s poll must not blank the screen behind a spinner every time.
    const state = stepAsync(loaded("a"), { kind: "start", generation: 2 });
    expect(viewAsync(state)).toMatchObject({ data: "a", loading: false, refreshing: true });
  });

  it("that fails keeps the data and marks it stale", () => {
    let state = stepAsync(loaded("a"), { kind: "start", generation: 2 });
    state = stepAsync(state, { kind: "failure", generation: 2, error: "unreachable" });
    expect(viewAsync(state)).toEqual({
      data: "a",
      error: "unreachable",
      loading: false,
      refreshing: false,
      stale: true,
    });
  });

  it("that succeeds after a failure clears the error and the staleness", () => {
    let state = stepAsync(loaded("a"), { kind: "start", generation: 2 });
    state = stepAsync(state, { kind: "failure", generation: 2, error: "unreachable" });
    state = stepAsync(state, { kind: "start", generation: 3 });
    state = stepAsync(state, { kind: "success", generation: 3, value: "b" });
    expect(viewAsync(state)).toEqual({
      data: "b",
      error: null,
      loading: false,
      refreshing: false,
      stale: false,
    });
  });

  it("that fails with nothing to show is an error, not stale data", () => {
    let state = stepAsync(initialAsync<string>(null), { kind: "start", generation: 1 });
    state = stepAsync(state, { kind: "failure", generation: 1, error: "unreachable" });
    expect(viewAsync(state)).toMatchObject({ data: null, error: "unreachable", stale: false });
  });
});

describe("the generation guard", () => {
  it("ignores a slow answer to a load that a newer one has replaced", () => {
    let state = stepAsync(initialAsync<string>(null), { kind: "start", generation: 1 });
    state = stepAsync(state, { kind: "start", generation: 2 });
    state = stepAsync(state, { kind: "success", generation: 2, value: "fresh" });
    state = stepAsync(state, { kind: "success", generation: 1, value: "slow and old" });
    expect(state.data).toBe("fresh");
  });

  it("ignores a late failure of a replaced load", () => {
    let state = stepAsync(loaded("a"), { kind: "start", generation: 2 });
    state = stepAsync(state, { kind: "success", generation: 2, value: "b" });
    state = stepAsync(state, { kind: "failure", generation: 1, error: "old" });
    expect(viewAsync(state)).toMatchObject({ data: "b", error: null, stale: false });
  });
});

describe("a change of dependencies", () => {
  it("drops the previous scope's data rather than showing it as a refresh", () => {
    // Agent A's detail must not be shown as a refresh of agent B's.
    const state = stepAsync(loaded("agent a"), { kind: "scope", seed: null });
    expect(viewAsync(state)).toMatchObject({ data: null, loading: true, refreshing: false });
  });

  it("starts from the new scope's seed when there is one", () => {
    const state = stepAsync(loaded("agent a"), { kind: "scope", seed: "agent b, cached" });
    expect(viewAsync(state)).toMatchObject({ data: "agent b, cached", refreshing: true });
  });
});
