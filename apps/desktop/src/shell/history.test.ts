import { describe, expect, it } from "vitest";

import { type HistoryHost, type Navigator, createNavigator, readDepth, stateFor } from "./history";

/**
 * A history that behaves like the browser's where it matters: pushing drops
 * the forward entries, and moving through it reports a pop afterwards rather
 * than during the call.
 */
class FakeHistory {
  entries: { state: unknown; hash: string }[];
  index = 0;
  answer = true;
  asked = 0;
  /** When set, questions wait until {@link answerHeld} is called. */
  holding = false;
  held: ((leave: boolean) => void) | null = null;
  navigator: Navigator | null = null;

  constructor(hash: string, state: unknown = null) {
    this.entries = [{ state, hash }];
  }

  get entry(): { state: unknown; hash: string } {
    const entry = this.entries[this.index];
    if (entry === undefined) throw new Error("no current entry");
    return entry;
  }

  host(): HistoryHost {
    return {
      hash: () => this.entry.hash,
      state: () => this.entry.state,
      push: (state, hash) => {
        this.entries.splice(this.index + 1, Infinity, { state, hash });
        this.index += 1;
      },
      replace: (state, hash) => {
        this.entries[this.index] = { state, hash };
      },
      go: (delta) => this.go(delta),
      confirmLeave: () => {
        this.asked += 1;
        if (!this.holding) return Promise.resolve(this.answer);
        return new Promise<boolean>((resolve) => {
          this.held = resolve;
        });
      },
    };
  }

  /** What the platform does for back, forward or the mouse's buttons. */
  go(delta: number): void {
    const target = this.index + delta;
    if (target < 0 || target >= this.entries.length) return;
    this.index = target;
    queueMicrotask(() => this.navigator?.popped());
  }

  /** A hash typed into the address bar: a new entry with no state. */
  type(hash: string): void {
    this.entries.splice(this.index + 1, Infinity, { state: null, hash });
    this.index += 1;
    queueMicrotask(() => this.navigator?.popped());
  }

  /** Answer the question that is being held open. */
  answerHeld(leave: boolean): void {
    const answer = this.held;
    if (answer === null) throw new Error("no question is open");
    this.held = null;
    answer(leave);
  }

  /** The depth the current entry records. */
  get depth(): number | null {
    return readDepth(this.entry.state);
  }

  start(): Navigator {
    this.navigator = createNavigator(this.host());
    return this.navigator;
  }
}

/** Let queued pops and guard answers run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
}

describe("createNavigator", () => {
  it("opens where the address says and rewrites it to the canonical form", () => {
    const history = new FakeHistory("#/agents/ops%2Fbilling");
    const nav = history.start();
    expect(nav.place()).toEqual({
      route: { name: "agents", agent: "ops/billing" },
      hash: "#/agents/ops%2Fbilling",
      depth: 0,
    });
    expect(readDepth(history.entry.state)).toBe(0);
  });

  it("opens a malformed link at the dashboard, showing the dashboard's address", () => {
    const history = new FakeHistory("#/agents/%E0%A4%A");
    const nav = history.start();
    expect(nav.place().route).toEqual({ name: "dashboard" });
    expect(history.entry.hash).toBe("#/dashboard");
  });

  it("writes one entry per move, each one deeper", async () => {
    const history = new FakeHistory("#/dashboard");
    const nav = history.start();
    await expect(nav.navigate({ name: "tasks" })).resolves.toBe(true);
    await expect(nav.navigate({ name: "tasks", runId: "r1" })).resolves.toBe(true);
    expect(history.entries.map((entry) => entry.hash)).toEqual([
      "#/dashboard",
      "#/tasks",
      "#/tasks/run/r1",
    ]);
    expect(nav.place().depth).toBe(2);
  });

  it("does not move to where it already is", async () => {
    const history = new FakeHistory("#/settings");
    const nav = history.start();
    await expect(nav.navigate({ name: "settings" })).resolves.toBe(false);
    expect(history.entries).toHaveLength(1);
    expect(history.asked).toBe(0);
  });

  it("stays, writing nothing, when the person chooses to keep their work", async () => {
    const history = new FakeHistory("#/agents/a");
    const nav = history.start();
    history.answer = false;
    await expect(nav.navigate({ name: "dashboard" })).resolves.toBe(false);
    expect(history.entries).toHaveLength(1);
    expect(nav.place().route).toEqual({ name: "agents", agent: "a" });
  });

  it("goes back to the previous screen and its depth", async () => {
    const history = new FakeHistory("#/dashboard");
    const nav = history.start();
    await nav.navigate({ name: "tasks" });
    nav.back();
    await settle();
    expect(nav.place()).toMatchObject({ route: { name: "dashboard" }, depth: 0 });
    nav.forward();
    await settle();
    expect(nav.place()).toMatchObject({ route: { name: "tasks" }, depth: 1 });
  });

  it("never goes back out of the app", async () => {
    const history = new FakeHistory("#/dashboard");
    history.entries.unshift({ state: null, hash: "" });
    history.index = 1;
    const nav = history.start();
    nav.back();
    await settle();
    expect(history.index).toBe(1);
    expect(nav.place().route).toEqual({ name: "dashboard" });
  });

  it("steps the history back to where it was when back is refused", async () => {
    const history = new FakeHistory("#/dashboard");
    const nav = history.start();
    await nav.navigate({ name: "agents" });
    await nav.navigate({ name: "agents", agent: "a" });
    history.answer = false;
    history.asked = 0;

    // The mouse's back button: the platform moves first and reports after.
    history.go(-2);
    await settle();

    expect(history.index).toBe(2);
    expect(history.entry.hash).toBe("#/agents/a");
    expect(nav.place()).toMatchObject({ route: { name: "agents", agent: "a" }, depth: 2 });
    expect(history.asked).toBe(1);
  });

  it("knows how deep it is after a reload", async () => {
    const history = new FakeHistory("#/dashboard");
    const nav = history.start();
    await nav.navigate({ name: "tasks" });
    await nav.navigate({ name: "activity" });

    const reloaded = history.start();
    expect(reloaded.place().depth).toBe(2);
    reloaded.back();
    await settle();
    expect(reloaded.place().route).toEqual({ name: "tasks" });
  });

  it("gives a typed address a depth, and replaces it when leaving is refused", async () => {
    const history = new FakeHistory("#/dashboard");
    const nav = history.start();
    history.type("#/settings");
    await settle();
    expect(nav.place()).toMatchObject({ route: { name: "settings" }, depth: 1 });
    expect(readDepth(history.entry.state)).toBe(1);

    history.answer = false;
    history.type("#/agents");
    await settle();
    expect(nav.place().route).toEqual({ name: "settings" });
    expect(history.entry).toEqual({ state: stateFor(1), hash: "#/settings" });
  });

  it("ignores an entry state it did not write", () => {
    expect(readDepth(null)).toBeNull();
    expect(readDepth({})).toBeNull();
    expect(readDepth({ agentosDepth: -1 })).toBeNull();
    expect(readDepth({ agentosDepth: 1.5 })).toBeNull();
    expect(readDepth({ agentosDepth: "2" })).toBeNull();
    expect(readDepth(stateFor(3))).toBe(3);
  });
  describe("a move while a leave question is open", () => {
    /** dashboard -> tasks -> agents/a, with the guard question held open. */
    async function deep(): Promise<{ history: FakeHistory; nav: Navigator }> {
      const history = new FakeHistory("#/dashboard");
      const nav = history.start();
      await nav.navigate({ name: "tasks" });
      await nav.navigate({ name: "agents", agent: "a" });
      history.holding = true;
      return { history, nav };
    }

    it("is undone when the person stays, so the address matches the screen", async () => {
      const { history, nav } = await deep();
      const moving = nav.navigate({ name: "dashboard" });
      // The mouse's back button, while the dialog is up.
      history.go(-1);
      await settle();
      history.answerHeld(false);
      await expect(moving).resolves.toBe(false);
      await settle();

      expect(nav.place()).toMatchObject({ route: { name: "agents", agent: "a" }, depth: 2 });
      expect(history.entry.hash).toBe("#/agents/a");
      expect(history.depth).toBe(2);
      // Back is one screen back, not two.
      history.holding = false;
      nav.back();
      await settle();
      expect(nav.place()).toMatchObject({ route: { name: "tasks" }, depth: 1 });
    });

    it("leaves from where the history is when the person leaves", async () => {
      const { history, nav } = await deep();
      const moving = nav.navigate({ name: "settings" });
      history.go(-1);
      await settle();
      history.answerHeld(true);
      await expect(moving).resolves.toBe(true);
      await settle();

      expect(nav.place()).toMatchObject({ route: { name: "settings" }, depth: 2 });
      expect(history.entry.hash).toBe("#/settings");
      expect(history.depth).toBe(2);
      expect(history.index).toBe(2);
      expect(history.entries.map((entry) => entry.hash)).toEqual([
        "#/dashboard",
        "#/tasks",
        "#/settings",
      ]);
    });

    it("gives an address typed meanwhile a depth before leaving past it", async () => {
      const { history, nav } = await deep();
      const moving = nav.navigate({ name: "settings" });
      history.type("#/approvals");
      await settle();
      history.answerHeld(true);
      await moving;
      await settle();

      expect(nav.place()).toMatchObject({ route: { name: "settings" }, depth: 4 });
      expect(history.depth).toBe(4);
      expect(readDepth(history.entries[3]!.state)).toBe(3);
    });

    it("does not open a second question for a second move", async () => {
      const { history, nav } = await deep();
      const asked = history.asked;
      const first = nav.navigate({ name: "dashboard" });
      await expect(nav.navigate({ name: "settings" })).resolves.toBe(false);
      expect(history.asked).toBe(asked + 1);
      history.answerHeld(true);
      await expect(first).resolves.toBe(true);
      expect(nav.place()).toMatchObject({ route: { name: "dashboard" }, depth: 3 });
    });
  });
});
