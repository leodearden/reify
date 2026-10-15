# GUI vitest: shared fork workers (task 7642)

The gui suite (`gui/vitest.config.ts`) reuses a handful of fork workers across
test files instead of spawning one cold fork per file. This note covers the
mechanism, why a per-file hook is needed, and the rules for gui test authors.
Measured against vitest **3.2.4**; a vitest major upgrade (v4 reworks pools and
`isolate`) must revisit all of it.

## Mechanism

- **Root `poolOptions.forks.isolate: false`.** Workers are reused. vitest's
  forks pool then groups files by project and environment, and recycles the
  workers between groups (`vitest/dist/chunks/coverage.*.js`, forks pool,
  non-isolated branch: `groupBy(project.name + environment.name …)` then
  `pool.recycleWorkers()`). A worker therefore never serves two projects.
- **Two projects decide whether a file also shares the worker's module graph.**
  - `shared-modules` (project `isolate: false`): an explicit `include` list of
    test files with no module-registry `vi` call. They share cached modules.
  - `fresh-modules` (project `isolate: true`): every other test file. vitest
    resets the mocker and the module cache before each file, inside the reused
    worker (`runBaseTests.*.js`: `executor.mocker.reset();
    resetModules(moduleCache, true)`).
- **The routing rule is owned by `gui/vitest-project-partition.ts`**: a file
  goes to `shared-modules` only if its own source contains none of `vi.mock`,
  `vi.doMock`, `vi.unmock`, `vi.doUnmock`, `vi.resetModules`, `vi.importMock`,
  `vi.importActual`. The fail-safe direction is `fresh-modules`: a file the walk
  misses, or a comment that merely mentions `vi.mock(`, costs speed, never
  correctness.
- **Even with `isolate: false`, jsdom is fresh per file.** The pool still calls
  `runFiles([file])` per file, and each call builds a new environment.

## Why the per-file hook exists

A reused worker keeps externalized libraries cached, and three of them capture
the FIRST file's `document` at import time:

1. `@testing-library/dom`'s `screen` binds `document.body` at import
   (`dist/screen.js:48`).
2. `@solidjs/testing-library` registers `afterEach(cleanup)` at import
   (`dist/index.js:17`), so only the first file in a worker would get it.
3. `solid-js/web`'s `delegateEvents(...)` runs at module evaluation of every
   compiled component and binds that file's document (`web/dist/dev.js:219`), so
   clicks in later files would never reach their handlers.

`gui/vitest.setup.ts` undoes these in its **body**, because vitest re-runs a
setup file once per test file but keeps the setup file's imports cached
(`vitest/dist/runners.js:109,133`: `if (source === "setup")
moduleCache.delete(filepath)`). It calls `rebindDomCapturesTo(document)`
(re-point `screen`, delegate every one of solid's `DelegatedEvents` on this
document), registers `afterEach(cleanup)`, and registers
`afterAll(restoreGlobalBaseline)` (real timers, `vi.unstubAllGlobals()`,
`vi.unstubAllEnvs()`). Both functions live in `gui/vitest-shared-worker.ts`
and have no module-scope side effects. `restoreGlobalBaseline` runs in
`afterAll`, not at the next file's start: unstubbing then would write the
previous jsdom's values over the new environment.

## Rules for gui test authors

- **Write module-registry `vi` calls in the test file itself.** That is what
  routes the file to `fresh-modules`. A helper module that calls `vi.mock` is
  invisible to the classifier.
- **In `shared-modules`, do not rely on fresh module-level state.** A module
  imported by an earlier file in the same worker is already evaluated.
- **Remove globals you set yourself** (`window.foo = …`, `globalThis.bar = …`)
  in teardown. Under vitest's jsdom environment `window` is Node's
  `globalThis`, so it outlives the file. `vi.stubGlobal` / `vi.stubEnv` /
  fake timers are restored for you.
- **A synthetic event's overridden `target` must be `configurable: true`.**
  solid's delegated document listener redefines `target` during dispatch and
  restores it afterwards; a non-configurable override throws
  `Cannot redefine property: target` (see `MenuBar.test.tsx`).
- **In watch mode**, a file that gains a `vi.mock` call is re-routed only after
  a restart: the partition is computed when the config loads.

## Measured facts

- Setup files are still fetched once per test file, and `fresh-modules` files
  still re-fetch their module graphs, so the worker→host fetch-RPC count falls
  only about 10% (2592 → 2133). The 60 s birpc bound described in
  `docs/notes/verify-pipeline-knobs.md` (GUI worker-RPC starvation marker)
  still applies; its exposure window shrinks.
- The win is about 4 reused workers (verify lane, `maxForks: 4`) instead of one
  cold fork per file. That removes most of the environment and prepare time
  and cuts CPU about 3x. Back-to-back under the same load: isolate=true 2:07
  wall and 287 s CPU, shared workers 0:35 wall and 89 s CPU, 190 files / 6132
  tests either way.
- Validation protocol used at landing: default order, five
  `--sequence.shuffle.files` seeds, `maxForks=1` shuffled (maximal sharing),
  and `DF_VERIFY_ROLE` unset. Any order-dependent failure is a real cross-file
  leak: fix it at its source, never by weakening the routing rule.
