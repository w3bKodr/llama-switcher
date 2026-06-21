import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and serves the frontend from there during dev.
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],

  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1431,
        }
      : undefined,
    watch: {
      // Don't watch the Rust backend.
      ignored: ["**/src-tauri/**"],
    },
  },

  // Produce a small, modern bundle for the system WebView.
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});
