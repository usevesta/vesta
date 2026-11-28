import { fileURLToPath, URL } from 'node:url'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

/** `pnpm tauri dev` runs this dev server for the app (Tauri sets `TAURI_ENV_*`). */
const RUN_BY_TAURI = process.env.TAURI_ENV_PLATFORM !== undefined

/**
 * Where the hub listens during development, so both loops can run at once:
 * - `pnpm dev` (browser): `vesta-hubd --dev` on `127.0.0.1:8080`.
 * - `pnpm tauri dev` (the app): its in-process hub on `127.0.0.1:8181`, with
 *   this server on port 1420 (`devUrl` in `apps/tauri/src-tauri/tauri.conf.json`).
 * Override with `VESTA_HUB_ORIGIN` (e.g. a hub started with `--port 9000`);
 * the app reads it too.
 */
const HUB_DEV_ORIGIN =
  process.env.VESTA_HUB_ORIGIN ?? (RUN_BY_TAURI ? 'http://127.0.0.1:8181' : 'http://127.0.0.1:8080')

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  // Keep the app's Rust logs on screen under `pnpm tauri dev`.
  clearScreen: !RUN_BY_TAURI,
  build: {
    rolldownOptions: {
      output: {
        // Third-party code in its own chunk: the app's own code stays small
        // and a screen keeps the libraries cached across app updates.
        codeSplitting: { groups: [{ name: 'vendor', test: /node_modules/ }] },
      },
    },
  },
  server: {
    ...(RUN_BY_TAURI ? { port: 1420, strictPort: true } : {}),
    proxy: {
      // HTTP base is `/api/v1`; the WebSocket lives at `/api/v1/ws`.
      '/api': {
        target: HUB_DEV_ORIGIN,
        changeOrigin: true,
        ws: true,
      },
    },
  },
  preview: {
    proxy: {
      '/api': {
        target: HUB_DEV_ORIGIN,
        changeOrigin: true,
        ws: true,
      },
    },
  },
})
