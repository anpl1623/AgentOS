#!/usr/bin/env node
/**
 * Capture one screenshot per screen of the desktop interface.
 *
 * The interface is served by a Vite dev server started in-process. Outside the
 * Tauri window a development build answers every runtime command from fixtures
 * (src/sdk/fixtures.ts), so the screens render their demo state without the
 * application running. The pages are photographed by the system's own Chrome in
 * headless mode: its `--screenshot` flag does the whole job, which keeps this
 * script free of a browser-automation dependency and the hundred-odd megabytes
 * of bundled Chromium that would come with one.
 *
 * Usage: npm run screenshots -- [--out <dir>] [--routes dashboard,approvals]
 *
 * Output is written to apps/desktop/screenshots/ by default, which is ignored
 * by git. Set CHROME_PATH to use a browser this script does not find itself.
 */

import { spawn } from "node:child_process";
import { accessSync, constants, mkdirSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { createServer } from "vite";

/**
 * Every screen, in navigation order. Each is loaded as `<url>/#/<route>`, the
 * address the shell's hash router gives that screen.
 */
const ROUTES = ["dashboard", "approvals", "tasks", "schedules", "agents", "activity", "settings"];

/** Logical viewport; the device scale factor doubles it in the PNG. */
const WINDOW_SIZE = "1280,860";

/**
 * Milliseconds of virtual time Chrome grants the page before capturing. Virtual
 * time pauses while requests are in flight, so this is time for React to render
 * and fixture promises to settle, not a wall-clock wait on the dev server.
 */
const VIRTUAL_TIME_BUDGET = 5000;

/** Wall-clock ceiling on a single capture, so a wedged browser cannot hang the run. */
const CAPTURE_TIMEOUT_MS = 60_000;

/** Grace given to a browser asked to quit before it is killed outright. */
const KILL_GRACE_MS = 5000;

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function fail(message) {
  console.error(`screenshots: ${message}`);
  process.exit(1);
}

function parseOptions() {
  let parsed;
  try {
    parsed = parseArgs({
      options: {
        out: { type: "string" },
        routes: { type: "string" },
      },
      strict: true,
      allowPositionals: false,
    });
  } catch (error) {
    fail(error.message);
  }
  const { out, routes } = parsed.values;

  let selected = ROUTES;
  if (routes !== undefined) {
    selected = routes
      .split(",")
      .map((route) => route.trim())
      .filter((route) => route.length > 0);
    // A mistyped route would otherwise produce a confident screenshot of the
    // dashboard under the wrong name.
    const unknown = selected.filter((route) => !ROUTES.includes(route));
    if (unknown.length > 0) {
      fail(`unknown route ${unknown.join(", ")}; known routes are ${ROUTES.join(", ")}`);
    }
    if (selected.length === 0) {
      fail("--routes named no routes");
    }
  }

  return {
    outDir: out === undefined ? join(appDir, "screenshots") : resolve(out),
    routes: selected,
  };
}

function isExecutable(path) {
  try {
    accessSync(path, constants.X_OK);
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

/** Find a Chrome or Chromium binary, preferring an explicit CHROME_PATH. */
function findChrome() {
  if (process.env.CHROME_PATH) {
    if (isExecutable(process.env.CHROME_PATH)) {
      return process.env.CHROME_PATH;
    }
    fail(`CHROME_PATH is set to ${process.env.CHROME_PATH}, which is not an executable file`);
  }

  const candidates = [];
  if (process.platform === "darwin") {
    const bundles = [
      "Google Chrome.app/Contents/MacOS/Google Chrome",
      "Chromium.app/Contents/MacOS/Chromium",
      "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    ];
    for (const root of ["/Applications", join(process.env.HOME ?? "", "Applications")]) {
      for (const bundle of bundles) {
        candidates.push(join(root, bundle));
      }
    }
  } else if (process.platform === "win32") {
    const roots = [
      process.env.PROGRAMFILES,
      process.env["PROGRAMFILES(X86)"],
      process.env.LOCALAPPDATA,
    ].filter(Boolean);
    for (const root of roots) {
      candidates.push(join(root, "Google", "Chrome", "Application", "chrome.exe"));
      candidates.push(join(root, "Chromium", "Application", "chrome.exe"));
    }
  } else {
    // Linux distributions disagree on the name, and snaps and flatpaks put the
    // binary wherever they like, so search PATH rather than fixed locations.
    const names = ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"];
    for (const dir of (process.env.PATH ?? "").split(delimiter).filter(Boolean)) {
      for (const name of names) {
        candidates.push(join(dir, name));
      }
    }
  }

  const found = candidates.find(isExecutable);
  if (found === undefined) {
    fail("no Chrome or Chromium found; install one or set CHROME_PATH to its binary");
  }
  return found;
}

/** The browser currently capturing, so an interrupted run can end it too. */
let activeBrowser = null;

function stopBrowser(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill();
  setTimeout(() => {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }, KILL_GRACE_MS).unref();
}

function fileSize(path) {
  try {
    return statSync(path).size;
  } catch {
    return 0;
  }
}

/**
 * Photograph one URL into `file`.
 *
 * Headless Chrome writes the screenshot once the virtual-time budget is spent,
 * but it does not reliably exit afterwards: on macOS it has been seen to linger
 * for a minute or indefinitely with the page still open. So the written file,
 * not the process exit, is the signal that the capture is done. The file is
 * taken as complete once its size holds steady across two polls, and Chrome is
 * then ended here rather than waited for.
 */
function capture(chrome, url, file, profileDir) {
  const args = [
    "--headless=new",
    `--screenshot=${file}`,
    `--window-size=${WINDOW_SIZE}`,
    "--hide-scrollbars",
    "--force-device-scale-factor=2",
    `--virtual-time-budget=${VIRTUAL_TIME_BUDGET}`,
    // A throwaway profile keeps the capture away from the user's own browser
    // state, and lets it run while their Chrome is open.
    `--user-data-dir=${profileDir}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    url,
  ];

  return new Promise((resolvePromise, reject) => {
    const child = spawn(chrome, args, { stdio: ["ignore", "ignore", "pipe"] });
    activeBrowser = child;
    let stderr = "";
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    let outcome = null;
    let lastSize = 0;
    const started = Date.now();

    const finish = (error) => {
      if (outcome !== null) return;
      outcome = error ?? "done";
      clearInterval(poll);
      stopBrowser(child);
    };

    const poll = setInterval(() => {
      const size = fileSize(file);
      if (size > 0 && size === lastSize) {
        finish();
      } else if (Date.now() - started > CAPTURE_TIMEOUT_MS) {
        finish(new Error(`Chrome did not capture ${url} within ${CAPTURE_TIMEOUT_MS / 1000}s`));
      }
      lastSize = size;
    }, 250);

    child.on("error", (error) => {
      finish(error);
      // A binary that never started emits no exit event to settle on.
      if (child.pid === undefined) reject(error);
    });
    // Settle only once the process is gone, so the next capture never starts
    // while this browser still holds the port or the profile.
    child.on("exit", () => {
      activeBrowser = null;
      if (outcome === null) {
        // Chrome left on its own; whatever it wrote is all there will be.
        finish(
          fileSize(file) > 0
            ? undefined
            : new Error(`Chrome exited without writing ${file}\n${stderr}`),
        );
      }
      if (outcome === "done") {
        resolvePromise();
      } else {
        reject(outcome);
      }
    });
  });
}

async function main() {
  const { outDir, routes } = parseOptions();
  // Look for the browser before starting anything that would need tearing down.
  const chrome = findChrome();
  mkdirSync(outDir, { recursive: true });

  const server = await createServer({
    root: appDir,
    configFile: join(appDir, "vite.config.ts"),
    logLevel: "warn",
    // Port 0 takes any free port. The config's fixed port exists for Tauri, and
    // holding to it here would fail whenever `npm run dev` is already running.
    server: { port: 0, strictPort: false, hmr: false },
  });
  const profileRoot = mkdtempSync(join(tmpdir(), "agentos-screenshots-"));

  // Vite installs its own SIGTERM handler, which closes the server and calls
  // process.exit before any cleanup of ours could be awaited. The browser and
  // the scratch profiles are therefore released from the exit hook, which runs
  // synchronously however the process ends.
  process.once("exit", () => {
    if (activeBrowser !== null) stopBrowser(activeBrowser);
    rmSync(profileRoot, { recursive: true, force: true });
  });
  for (const [signal, number] of [
    ["SIGINT", 2],
    ["SIGTERM", 15],
  ]) {
    process.once(signal, () => {
      void server.close().finally(() => process.exit(128 + number));
    });
  }

  try {
    await server.listen();
    const base = server.resolvedUrls?.local[0];
    if (base === undefined) {
      throw new Error("the dev server did not report a local URL");
    }

    for (const route of routes) {
      const file = join(outDir, `${route}.png`);
      rmSync(file, { force: true });
      // A profile per capture: a browser that is slow to release its profile
      // lock must not turn the next launch into a hand-off to itself.
      await capture(chrome, `${base}#/${route}`, file, join(profileRoot, route));
      console.log(file);
    }
  } finally {
    await server.close();
  }
}

main().catch((error) => {
  console.error(`screenshots: ${error.message}`);
  process.exit(1);
});
