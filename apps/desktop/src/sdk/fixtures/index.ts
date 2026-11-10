/**
 * Development fixtures.
 *
 * These answer runtime commands when the interface runs in an ordinary browser,
 * so screens can be built and reviewed without launching the application. They
 * are reached only when `import.meta.env.DEV` is true, which Vite replaces with
 * a literal at build time — so the branch, and everything it references, is dead
 * code in a production bundle.
 *
 * Dead code is not enough on its own: the bundler keeps any top-level statement
 * it cannot prove free of side effects, such as a spread or a `.map` over a
 * table, and the data that statement reads with it. The `package.json` in this
 * folder declares these modules free of side effects, so a production build,
 * which never calls into them, drops them whole.
 *
 * The data deliberately mirrors the shipped demo: an agent that has read a CRM
 * record containing a prompt-injection payload, and is now asking to do
 * something consequential. That is the state the approval card exists for, so it
 * is the state worth being able to look at.
 */

// Each module below holds one part of the demo state and the commands that
// answer from it; `dispatch.ts` routes a command to its answer.

export { fixtureSubscribe } from "./activity";
export { fixtureInvoke } from "./dispatch";
