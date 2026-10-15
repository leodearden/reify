# GUI vitest: shared fork workers (task 7642)

The gui suite (`gui/vitest.config.ts`) reuses a handful of fork workers across
test files instead of spawning one cold fork per file. This note covers the
mechanism, why a per-file hook is needed, and the rules for gui test authors.
Measured against vitest **3.2.4**; a vitest major upgrade (v4 reworks pools and
`isolate`) must revisit all of it.

## Mechanism

- **Root `poolOptions.forks.isolate: false`.** Workers are reused. vitest's
  forks pool then groups files by project and environment, and recycles the
  workers between groups (its non-isolated branch:
  `groupBy(… project.name + environment.name …)`, then
  `pool.recycleWorkers()`). A worker therefore never serves two projects.
- **Two projects decide whether a file also shares the worker's module graph.**
  - `shared-modules` (project `isolate: false`): an explicit `include` list of
    test files with no module-registry `vi` call. They share cached modules.
  - `fresh-modules` (project `isolate: true`): every other test file. vitest
    resets the mocker and the module cache before each file, inside the reused
    worker (`runBaseTests`: `executor.mocker.reset();
    resetModules(moduleCache, true)`).
- **The routing rule is `MODULE_REGISTRY_CALL` in
  `gui/vitest-project-partition.ts`**: a file goes to `shared-modules` only if
  its own source matches none of the `vi` APIs listed there. The fail-safe
  direction is `fresh-modules`: a file the walk misses, or a comment that
  merely mentions such a call, costs speed, never correctness.
- **Even with `isolate: false`, jsdom is fresh per file.** The pool still calls
  `runFiles([file])` per file, and each call builds a new environment.

## Why the per-file hook exists

A reused worker keeps externalized libraries cached, and three of them capture
the FIRST file's `document` at import time:

1. `@testing-library/dom` builds its exported `screen` from `document.body`
   when that module is evaluated.
2. `@solidjs/testing-library` registers `afterEach(cleanup)` when its entry
   module is evaluated, so only the first file in a worker would get it.
   `gui/vitest.config.ts` sets `STL_SKIP_AUTO_CLEANUP` in `test.env` to turn
   that hook off, which leaves the setup body's registration as the only one.
3. Every compiled component calls `solid-js/web`'s
   `delegateEvents(eventNames, document = window.document)` at module
   evaluation, which binds that file's document. Clicks in later files would
   never reach their handlers.

`gui/vitest.setup.ts` undoes these in its **body**, because vitest re-runs a
setup file once per test file but keeps the setup file's imports cached
(`VitestTestRunner.importFile`: `if (source === "setup")
moduleCache.delete(filepath)`, which drops only the setup file itself). It
calls `rebindDomCapturesTo(document)` (re-point `screen`, delegate every one
of solid's `DelegatedEvents` on this document), registers
`afterEach(cleanup)`, and registers `afterAll(restoreGlobalBaseline)` (real
timers, `vi.unstubAllGlobals()`, `vi.unstubAllEnvs()`,
`vi.restoreAllMocks()`). Both functions live in `gui/vitest-shared-worker.ts`
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
  fake timers / `vi.spyOn` spies are restored for you.
- **Create `vi.fn()` mocks inside a factory or a test, never at module scope
  of a shared helper.** In vitest 3.2.4 `vi.restoreAllMocks()` resets every
  `vi.fn()`, not only spies, to the implementation it was created with. A
  cached helper's module-scope mock would reach the next file without the
  `mockReturnValue` / `mockImplementation` it was configured with.
- **A synthetic event's overridden `target` must be `configurable: true`.**
  solid's delegated document listener redefines `target` during dispatch and
  restores it afterwards; a non-configurable override throws
  `Cannot redefine property: target` (see `MenuBar.test.tsx`).
- **In watch mode**, a file that gains a `vi.mock` call is re-routed only after
  a restart: the partition is computed when the config loads.

## Residual risk

Setup files are still fetched once per test file, and `fresh-modules` files
still re-fetch their module graphs, so worker→host fetch-RPC volume falls only
slightly. The 60 s birpc bound described in `docs/notes/verify-pipeline-knobs.md`
(GUI worker-RPC starvation marker) still applies; reused workers only shrink
its exposure window.

## Re-validating (after a vitest upgrade or a change to the config wiring)

1. **Project wiring.** From `gui/`, run
   `./node_modules/.bin/vitest list --filesOnly --json=<out.json>`. Each entry
   is `{ file, projectName }`. Check that every file appears exactly once, that
   no file under `sidecar/` or `node_modules/` appears, and that every file for
   which `needsFreshModuleGraph` (from `gui/vitest-project-partition.ts`) is
   true has `projectName` `fresh-modules`. Nothing in the gate checks this. A
   change in how vitest merges a project's `include` / `exclude` with the root
   config under `extends: true` would surface only as order-dependent flakes,
   or as files silently dropped from both projects.
2. **Order independence.** Run the suite in default order, with five
   `--sequence.shuffle.files` seeds, with `maxForks=1` plus shuffling (maximal
   sharing), and with `DF_VERIFY_ROLE` unset. Any order-dependent failure is a
   real cross-file leak: fix it at its source, never by weakening the routing
   rule.
