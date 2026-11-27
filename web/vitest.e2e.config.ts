import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vitest/config'

/** End-to-end tests against a real `vesta-hubd --dev` (see e2e/hub-smoke.e2e.ts). */
export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    include: ['e2e/**/*.e2e.ts'],
    environment: 'node',
    fileParallelism: false,
    testTimeout: 30_000,
    // Starting the hub may include building it with cargo.
    hookTimeout: 600_000,
  },
})
