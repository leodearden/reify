import { defineConfig } from 'vitest/config';
import solidPlugin from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solidPlugin()],
  test: {
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
