/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  // Tauri prints its own output; clearing the screen hides it.
  clearScreen: false,
  server: {
    port: 1420,
    // A shifting port would leave tauri.conf.json pointing at nothing.
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
  test: {
    // The unit tests cover pure logic, so they run in plain Node. A simulated
    // DOM would be a second, subtly different browser to keep in step with the
    // real webview; rendering is checked with real Chrome by the screenshot
    // script instead.
    environment: "node",
    include: ["src/**/*.test.ts"],
    // Stylesheets load as empty strings by default. A test that holds a
    // TypeScript vocabulary to the stylesheet rendering it — alert levels to
    // `.alert.<level>` — imports the sheet with `?raw` and needs its text.
    css: true,
  },
});
