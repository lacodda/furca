import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  test: {
    include: ['src/**/*.test.ts'],
    // The scaffold ships no product logic yet, so there is nothing to unit
    // test — a real suite arrives with the first feature. Failing CI on zero
    // tests would only be noise until then.
    passWithNoTests: true,
  },
})
