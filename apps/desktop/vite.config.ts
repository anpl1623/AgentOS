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
});
