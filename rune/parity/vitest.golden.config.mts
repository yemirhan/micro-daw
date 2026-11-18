/**
 * Golden-fixture generator: runs TypeScript dumps of the Electron app's pure code and writes
 * JSON fixtures that the Rust tests compare against ("same inputs → same outputs").
 *
 *   npx vitest run --config rune/parity/vitest.golden.config.mts
 *
 * Each `rune/parity/ts/*.golden.ts` file imports modules from `src/` (same alias and Tone mock
 * as `npm test`) and calls `writeFixture(name, data)`.
 */
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  root: fileURLToPath(new URL('../..', import.meta.url)),
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('../../src', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    globals: false,
    include: ['rune/parity/ts/**/*.golden.ts'],
    setupFiles: ['./src/test/setup.ts'],
    restoreMocks: true,
    // The golden dumps drive whole engine sessions (thousands of steps): far beyond the 5 s default.
    testTimeout: 60000,
  },
});
