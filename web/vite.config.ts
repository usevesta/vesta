import { fileURLToPath, URL } from 'node:url'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

/**
 * Where `vesta-hubd` listens during development. Override with
 * `VESTA_HUB_ORIGIN` (e.g. a hub started with `--port 9000`).
 */
const HUB_DEV_ORIGIN = process.env.VESTA_HUB_ORIGIN ?? 'http://127.0.0.1:8080'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
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
