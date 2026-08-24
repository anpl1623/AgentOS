/**
 * How the interface reaches the runtime.
 *
 * Inside the desktop window this is Tauri's IPC. Outside it — running `npm run
 * dev` in an ordinary browser — there is no runtime to reach, so a fixture
 * transport answers instead. That exists so screens can be built and reviewed
 * without launching the whole application, and it is confined to development
 * builds: a production bundle that could serve invented data would be a way to
 * show somebody an approval that never happened.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

import { fixtureInvoke, fixtureSubscribe } from "./fixtures";

/** Whether the interface is running inside the desktop window. */
export function inDesktopWindow(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Whether answers are coming from fixtures rather than the runtime. */
export function usingFixtures(): boolean {
  return !inDesktopWindow() && import.meta.env.DEV;
}

/** Invoke a runtime command. */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (inDesktopWindow()) {
    return tauriInvoke<T>(command, args);
  }
  if (import.meta.env.DEV) {
    return fixtureInvoke<T>(command, args);
  }
  throw new Error(
    `Cannot reach the AgentOS runtime: \`${command}\` was called outside the desktop window.`,
  );
}

/** Subscribe to a runtime event. Returns an unsubscribe function. */
export async function subscribe<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (inDesktopWindow()) {
    const unlisten = await tauriListen<T>(event, ({ payload }) => handler(payload));
    return unlisten;
  }
  if (import.meta.env.DEV) {
    return fixtureSubscribe<T>(event, handler);
  }
  return () => {};
}
