/// <reference types="vitest" />
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'node:path';

/**
 * v0.4.22 (event 000118, fix 5 hardening): vitest config for
 * the desktop app. We use jsdom for any component tests and
 * `node` env for the pure-function tests like `chatFallback`.
 *
 * Note: only loads Vite plugin `react()` for the JSX tests; pure
 * unit tests don't need it but importing it is cheap.
 */
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@flowntier/shared': path.resolve(__dirname, '../../packages/shared/src/index.ts'),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}'],
    globals: false,
  },
});
