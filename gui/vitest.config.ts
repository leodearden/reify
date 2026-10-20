import { fileURLToPath } from 'node:url';
import { defineConfig, type TestProjectInlineConfiguration } from 'vitest/config';
import solidPlugin from 'vite-plugin-solid';
import {
  FRESH_MODULES_PROJECT,
  listSharedModuleTestFiles,
  NON_GUI_TEST_DIRECTORIES,
  SHARED_MODULES_PROJECT,
} from './vitest-project-partition';

// FM2 mitigation (esc-4853-42 / task 4856): under verify-lane cargo
// contention the Node main process is starved for 60+ s, causing vitest's
// internal snapshotSaved worker→main birpc RPC to time out (birpc
// DEFAULT_TIMEOUT is hardcoded at 60 s; no config knob in vitest 3.x).
// Reducing concurrent fork workers from the default (nCPU-1 ≈ 31) to a
// small cap relieves the main process so it can respond within 60 s.
// Gated on DF_VERIFY_ROLE (task|merge), which verify.sh EXPORTS (the export is
// load-bearing — task 7630) and the orchestrator injects; local `npm test`
// leaves it unset and so retains full parallelism.
// teardownTimeout is also raised from the default 10 000 ms: under severe
// starvation, graceful worker shutdown takes longer than 10 s, and a
// force-kill during teardown compounds the RPC-timeout failures.
const isVerifyLane =
  process.env.DF_VERIFY_ROLE === 'task' || process.env.DF_VERIFY_ROLE === 'merge';

// Test files that may share a reused fork worker's module graph; every other
// test file gets a fresh module graph (task 7642, see
// docs/notes/gui-vitest-shared-workers.md).
const SHARED_MODULE_TEST_FILES = listSharedModuleTestFiles(
  fileURLToPath(new URL('.', import.meta.url)),
);

const sharedModulesProject: TestProjectInlineConfiguration = {
  extends: true,
  test: {
    name: SHARED_MODULES_PROJECT,
    include: SHARED_MODULE_TEST_FILES,
    poolOptions: { forks: { isolate: false } },
  },
};

const freshModulesProject: TestProjectInlineConfiguration = {
  extends: true,
  test: {
    name: FRESH_MODULES_PROJECT,
    exclude: SHARED_MODULE_TEST_FILES,
    poolOptions: { forks: { isolate: true } },
  },
};

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
    // ADDITIONAL to 'default', so human-readable output is unchanged. Registered
    // unconditionally rather than behind isVerifyLane: it is inert unless the
    // worker->host RPC starvation signature matches, and a local reproduction
    // should classify identically (task 7630).
    reporters: ['default', './vitest-worker-rpc-flake-reporter.ts'],
    // Raised from the default 10 000 ms so workers have time to complete
    // their teardown (including the snapshotSaved RPC) under verify-lane
    // starvation before the pool force-terminates them (esc-4853-42 / task 4856).
    teardownTimeout: isVerifyLane ? 120_000 : 10_000,
    environment: 'jsdom',
    globals: true,
    // Global jsdom Range rectangle-measurement polyfills (task #5361 /
    // esc-5361-2). jsdom's Range lacks getClientRects/getBoundingClientRect;
    // CodeMirror's drawSelection() (newly activated by the PRIMARY-selection
    // guard) measures selection rects via a Range and routes the resulting
    // throw to console.error, tripping ide-affordances.e2e.test.ts's
    // zero-console-error gate. The setup file installs standards-shaped stubs
    // only when absent. Must be a global setupFile: per-test jsdom isolation
    // means an in-test polyfill can't reach other test files.
    setupFiles: ['./vitest.setup.ts'],
    // Fails the run if the projects below do not run each gui test file in
    // exactly the project vitest-project-partition.ts assigns it (task 7642).
    // Only the root project's copy checks; the projects inherit it as a no-op.
    globalSetup: ['./vitest-partition-guard.ts'],
    // @solidjs/testing-library registers afterEach(cleanup) when it is
    // evaluated, which a shared-modules worker does only once. vitest.setup.ts
    // registers cleanup for every file instead; switching the import-time hook
    // off leaves that as the only registration (task 7642).
    env: { STL_SKIP_AUTO_CLEANUP: 'true' },
    exclude: NON_GUI_TEST_DIRECTORIES.map((directory) => `${directory}/**`),
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
    // Fork workers are reused across test files (isolate: false); whether a
    // file also gets a fresh module graph is decided per project below (task
    // 7642, see docs/notes/gui-vitest-shared-workers.md). The verify lane also
    // caps concurrent workers at 4 to relieve main-process starvation (FM2,
    // esc-4853-42 / task 4856): uncapped, nCPU-1 workers can all call
    // snapshotSaved while cross-worktree cargo builds starve the host past
    // the 60 s birpc timeout. Local dev (DF_VERIFY_ROLE unset) keeps full
    // parallelism.
    poolOptions: {
      forks: { isolate: false, ...(isVerifyLane ? { maxForks: 4 } : {}) },
    },
    projects:
      SHARED_MODULE_TEST_FILES.length === 0
        ? [freshModulesProject]
        : [sharedModulesProject, freshModulesProject],
  },
  resolve: {
    conditions: ['development', 'browser'],
  },
});
