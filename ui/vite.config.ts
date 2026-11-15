/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Vite config for the Tauri webview. See https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  // Keep Rust errors visible: Vite must not clear the terminal Tauri shares with it.
  clearScreen: false,
  server: {
    // Tauri's devUrl expects exactly this port; fail instead of picking another one.
    port: 5173,
    strictPort: true,
    // Rust files are watched by Tauri, not Vite.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Expose VITE_* and Tauri's TAURI_ENV_* variables to the UI.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
  },
});
