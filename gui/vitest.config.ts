import { defineConfig } from 'vitest/config';
import solidPlugin from 'vite-plugin-solid';

// FM2 mitigation (esc-4853-42 / task 4856): under verify-lane cargo
// contention the Node main process is starved for 60+ s, causing vitest's
// internal snapshotSaved worker→main birpc RPC to time out (birpc
// DEFAULT_TIMEOUT is hardcoded at 60 s; no config knob in vitest 3.x).
// Reducing concurrent fork workers from the default (nCPU-1 ≈ 31) to a
// small cap relieves the main process so it can respond within 60 s.
// Gated on DF_VERIFY_ROLE (task|merge, set by verify.sh:328 and injected
// by the orchestrator) so local `npm test` retains full parallelism.
// teardownTimeout is also raised from the default 10 000 ms: under severe
// starvation, graceful worker shutdown takes longer than 10 s, and a
// force-kill during teardown compounds the RPC-timeout failures.
const isVerifyLane =
  process.env.DF_VERIFY_ROLE === 'task' || process.env.DF_VERIFY_ROLE === 'merge';

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
    // Raised from the default 10 000 ms so workers have time to complete
    // their teardown (including the snapshotSaved RPC) under verify-lane
    // starvation before the pool force-terminates them (esc-4853-42 / task 4856).
    teardownTimeout: isVerifyLane ? 120_000 : 10_000,
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
    // Cap concurrent fork workers in the verify lane to relieve main-process
    // starvation (FM2, esc-4853-42 / task 4856). The forks pool is vitest's
    // default; without a cap it spawns nCPU-1 workers (≈31 on a 32-core box),
    // all of which can call snapshotSaved simultaneously while the main process
    // is starved by cross-worktree cargo builds, exceeding the 60 s birpc timeout.
    // 4 concurrent workers still gives meaningful parallelism while dramatically
    // reducing concurrent RPC pressure on the starved main process.
    // Local dev (DF_VERIFY_ROLE unset) is unaffected and keeps full parallelism.
    poolOptions: isVerifyLane
      ? { forks: { maxForks: 4 } }
      : {},
  },
  resolve: {
    conditions: ['development', 'browser'],
  },
});
