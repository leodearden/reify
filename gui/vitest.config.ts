import { defineConfig } from 'vitest/config';
import solidPlugin from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solidPlugin()],
  test: {
    // Raised from vitest defaults (5 000 ms / 10 000 ms) to absorb the
    // cargo-concurrency scheduling jitter that starves the Node event loop
    // when cross-worktree cargo workers saturate the 32-token jobserver
    // (esc-2915-17 / esc-3061-3, task 3185). These values replace the
    // per-test overrides that were previously scattered across test files.
    // hookTimeout raised from 30 000 to 90 000 to cover the viewport/index
    // cold-import (~2.3 s normally, but can balloon to >30 s under heavy
    // cross-worktree cargo load; esc-3061-3 class of jitter).
    testTimeout: 15_000,
    hookTimeout: 90_000,
    environment: 'jsdom',
    globals: true,
    exclude: ['sidecar/**', 'node_modules/**'],
    transformMode: {
      web: [/\.[jt]sx?$/],
    },
    server: {
      deps: {
        // vite-plugin-solid externals solid-js so Node resolves it.
        // Add 'development' + 'browser' conditions so Node picks dist/dev.js
        // (where createEffect works) instead of dist/server.js (no-op).
        conditions: ['development', 'browser'],
      },
    },
  },
  resolve: {
    conditions: ['development', 'browser'],
  },
});
