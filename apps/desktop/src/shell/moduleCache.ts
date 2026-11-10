/**
 * Modules fetched on first use, kept as promises React can read without
 * waiting once they have arrived.
 *
 * React's `use` returns a promise's value in the same render when the promise
 * is marked settled: the `status` and `value` fields React itself writes onto
 * a promise it waits for. A promise React has not seen before carries no mark,
 * so even one that resolved long ago makes the render suspend for a microtask
 * — and a suspended move commits its fallback, after which React holds the
 * finished render back for its fallback throttle (300ms). Marking each promise
 * here, as it settles, is what lets a module fetched in advance be drawn at
 * once.
 */

/** A promise with React's settled-state fields, written as it settles. */
export type Tracked<T> = Promise<T> &
  (
    | { status: "pending" }
    | { status: "fulfilled"; value: T }
    | { status: "rejected"; reason: unknown }
  );

/** `promise`, marked as React marks the promises it waits for. */
export function track<T>(promise: Promise<T>): Tracked<T> {
  const tracked = Object.assign(promise, { status: "pending" as const });
  promise.then(
    (value) => {
      Object.assign(promise, { status: "fulfilled", value });
    },
    (reason: unknown) => {
      Object.assign(promise, { status: "rejected", reason });
    },
  );
  return tracked;
}

/** A promise already marked with its value, for a module that has arrived. */
function settled<T>(value: T): Tracked<T> {
  return Object.assign(Promise.resolve(value), { status: "fulfilled" as const, value });
}

/** How to fetch each module in `M`, by name. */
export type Loaders<M> = { readonly [K in keyof M]: () => Promise<M[K]> };

/** Each module fetched once, and drawn without waiting once it is here. */
export interface ModuleCache<M> {
  /**
   * The module for `name`, as one promise kept for the life of the window.
   * Read it with `use`: a module already here is returned in the same render,
   * and one still on its way suspends until it arrives.
   *
   * A failed fetch stays failed, as `lazy` keeps one, so a render that waited
   * on it meets the failure and its error boundary says so rather than
   * fetching again in a loop.
   */
  read<K extends keyof M>(name: K): Tracked<M[K]>;
  /**
   * Fetch the module for `name` without drawing anything. It joins the cache
   * only once it has arrived; a failure is dropped, so the first draw fetches
   * again and meets any failure itself, where there is a boundary to show it.
   */
  preload(name: keyof M): Promise<void>;
}

/** A cache over `loaders`, one entry per name. */
export function createModuleCache<M>(loaders: Loaders<M>): ModuleCache<M> {
  const modules = new Map<keyof M, Tracked<unknown>>();
  return {
    read<K extends keyof M>(name: K): Tracked<M[K]> {
      let entry = modules.get(name);
      if (entry === undefined) {
        entry = track(loaders[name]());
        modules.set(name, entry);
      }
      return entry as Tracked<M[K]>;
    },
    async preload(name) {
      if (modules.has(name)) return;
      try {
        const module = await loaders[name]();
        // A draw that asked while this was on its way has its own promise,
        // which React is already waiting on; that one is kept.
        if (!modules.has(name)) modules.set(name, settled(module));
      } catch {
        // Left for the first draw to meet; see `preload` above.
      }
    },
  };
}
