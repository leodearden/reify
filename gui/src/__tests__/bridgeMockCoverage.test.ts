/**
 * Structural coverage guard (task-6053, reworked by task 10110):
 * runtime exports of bridge.ts ↔ keys of the shared App harness's `../bridge`
 * factory, `APP_BRIDGE_DEFAULTS` in appHarness/moduleMocks.ts.
 *
 * SINGLE SOURCE OF TRUTH for the App harness mock contract. The harness
 * modules carry a one-line pointer here rather than a copy: this narrative is
 * normative prose about a vitest mechanism, and lockstep copies would rot with
 * nothing to detect it — unlike the key sets below, which are checked.
 *
 * ── Invariant ────────────────────────────────────────────────────────────────
 * Every runtime export of `../bridge` is either a key of APP_BRIDGE_DEFAULTS or
 * carries a documented entry in the `omissions` allowlist, and every test file
 * that renders `<App />` installs the shared harness.
 *
 * ── Why an omitted key is dangerous rather than merely wrong ─────────────────
 * The factory is a NON-partial mock, so an omitted export makes vitest throw
 * `No "X" export is defined on the "../bridge" mock` SYNCHRONOUSLY at property
 * access. `initApp`'s run of sibling try/catch blocks then gives that ONE defect
 * two opposite signatures, neither of which fails an assertion:
 *   - export consumed by `engineStore.subscribeToEvents` -> a DOM toast and ZERO
 *     stderr. The throw fires while the `Promise.allSettled` argument array is
 *     still being built, so every engineStore subscription dies at once and
 *     allSettled's per-listener `console.warn` is never reached.
 *   - export consumed directly by an `initApp` block -> an stderr flood, with
 *     every test still green.
 * Tasks 6035, 6039 and 6045 each fixed one name of this class by hand, one
 * export at a time. Check (b) is still the only detector of the whole class:
 * a key absent from APP_BRIDGE_DEFAULTS is absent from the factory and from
 * the per-test priming alike, so nothing else notices it.
 *
 * ── Why no override can leak ─────────────────────────────────────────────────
 * `primeAppHarness` resets EVERY factory key to its table default before each
 * test (`mockReset().mockImplementation(default)`), dropping persistent and
 * queued *Once overrides alike. The old truth-table row "factory key present,
 * beforeEach restore missing -> an override leaks into every later test" can
 * therefore not occur, and needs no per-override detector.
 * appHarness.test.tsx pins that behaviour.
 *
 * ── Standing rules ───────────────────────────────────────────────────────────
 * 1. A new bridge.ts runtime export gets a key in APP_BRIDGE_DEFAULTS, OR an
 *    entry in `omissions` with the reason it is unreachable from a rendered
 *    <App />.
 * 2. Do NOT convert the factory to `importOriginal` / a partial mock. That
 *    would evaluate the real bridge.ts — its `@tauri-apps/api` and
 *    `@tauri-apps/plugin-dialog` imports and every real implementation — inside
 *    the most load-bearing mock in the suite.
 * 3. Defaults live only in APP_BRIDGE_DEFAULTS, typed by its `satisfies`
 *    clause so tsc checks every payload; a GuiState or PublishedState default
 *    is built from `makeGuiState()` / `published()`. A test overrides a
 *    default in its own body or its describe's beforeEach, never by editing
 *    the table for one test.
 * 4. A test file that renders <App /> writes the nine APP_HARNESS_INSTALL_LINES
 *    and `beforeEach(primeAppHarness)`.
 *
 * Shape mirrors `debugParity.test.ts` — the house pattern for "two artifacts
 * must stay in lockstep, with documented legitimate asymmetries": mock the
 * runtime deps, read both sides authoritatively, keep a named allowlist
 * carrying prose reasons, and add both a sanity check and an allowlist
 * self-check so neither side can rot into a rubber stamp.
 */
import { describe, it, expect, vi } from 'vitest';

// These three mocks make bridge.ts importable at collection time without a
// Tauri runtime. Proven sufficient: bridge.test.ts imports the same module with
// exactly this set. `@tauri-apps/plugin-dialog` is the reason this guard cannot
// live inside an App harness file — the harness does not mock it.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
  ask: vi.fn(),
}));

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';

// REAL module (the three mocks above only stub its Tauri dependencies).
import * as bridge from '../bridge';

import { APP_BRIDGE_DEFAULTS, APP_HARNESS_INSTALL_LINES } from './appHarness/moduleMocks';
import { missingFactoryKeys, staleOmissions } from './bridgeMockContract';

/**
 * Authoritative export list. `Object.keys` on the real ESM namespace needs no
 * regex and cannot drift: TS types erase, so this is exactly the set of runtime
 * exports a `vi.mock` factory has to satisfy. Do NOT replace with a source
 * parse of bridge.ts.
 */
const RUNTIME_EXPORTS = Object.keys(bridge);

/** The shared factory's keys, read from the table it is built from. */
const FACTORY_KEYS = Object.keys(APP_BRIDGE_DEFAULTS);

/**
 * Runtime exports the factory may legitimately omit, each with the reason it is
 * unreachable from a rendered `<App />` THROUGH THE MOCKED NAMESPACE. Check (c)
 * keeps every entry honest in the two directions it can: allowlisting a name
 * bridge.ts no longer exports, or one the factory in fact mocks, fails.
 *
 * TWO GRADES OF JUSTIFICATION — know which one you are relying on:
 *   MECHANISM (cannot rot): the only caller is inside bridge.ts itself. A
 *     same-module call resolves against the real binding, never the mocked
 *     namespace, so no edit to App.tsx or a component can make it reachable.
 *   PREMISE (can rot silently): "no non-test importer today". Nothing here
 *     checks that, so if a component starts importing the name, check (c) stays
 *     green — it only detects the two rot conditions above. Re-verify the
 *     importer set when you touch one of these, and prefer just adding the key
 *     to APP_BRIDGE_DEFAULTS when the export is function-shaped: three entries
 *     that rested on a premise about ../viewport being mocked were retired that
 *     way, because one table line beats a premise the guard cannot check.
 *     The four PREMISE entries that remain are the ones where a `vi.fn()` stub
 *     would be an active lie: two are data constants (a fake value could be
 *     silently asserted against, whereas the missing-export throw is loud and
 *     names itself), and two have no non-test caller at all.
 *
 * The bar for adding an entry is "no non-test importer can reach it through the
 * mocked namespace", NOT "adding the key is inconvenient". `claudePermissionDecision`
 * is deliberately NOT here: App.tsx imports it and calls it in `createClaudeStore`'s
 * `onPermissionDecision` handler, so it is genuinely reachable.
 */
const BRIDGE_INTERNAL_OMISSIONS: Record<string, string> = {
  BUILD_CONTEXT_HANDLED_FIELDS:
    'PREMISE — const array; only importers are __tests__/types.typecheck.ts and __tests__/claudeBridge.test.ts (the ChatPanel.tsx hit is a comment)',
  MESSAGE_CONTEXT_FIELD_MAP:
    'MECHANISM — const map iterated by mapContextToWire inside bridge.ts (the ChatPanel.tsx hit is a comment)',
  lspRequest:
    'PREMISE — no non-test importer at all; editor/lspClient.ts declares its OWN local lspRequest and calls invoke directly',
  mapContextToWire: 'MECHANISM — called only by claudeSendMessage within bridge.ts',
  onFeaCaseChanged: 'MECHANISM — called only by subscribeFeaCaseToStore within bridge.ts',
  refreshFullState: 'PREMISE — test-only; sole importer is __tests__/bridge.test.ts',
  validatePayload: "MECHANISM — called only from bridge.ts's own claude/sidecar listeners",
};

describe('bridge-mock coverage: bridge.ts runtime exports ↔ the shared App harness factory', () => {
  it('(a) sanity — neither side is vacuously empty, and every factory key is an export', () => {
    expect(RUNTIME_EXPORTS.length).toBeGreaterThanOrEqual(60);
    for (const name of [
      'getInitialState',
      'onModeShapeFrame',
      'syncDemand',
      'claudePermissionDecision',
    ]) {
      expect(RUNTIME_EXPORTS, `bridge.ts must export '${name}'`).toContain(name);
    }

    expect(FACTORY_KEYS.length).toBeGreaterThanOrEqual(55);
    expect(FACTORY_KEYS).toContain('getInitialState');
    expect(FACTORY_KEYS).toContain('onMeshUpdate');
    const notExports = FACTORY_KEYS.filter((k) => !RUNTIME_EXPORTS.includes(k));
    expect(notExports).toStrictEqual([]);
  });

  it('(b) every bridge.ts runtime export is mocked or documented as omitted', () => {
    expect(missingFactoryKeys(RUNTIME_EXPORTS, FACTORY_KEYS, BRIDGE_INTERNAL_OMISSIONS)).toStrictEqual([]);
  });

  it('(c) the allowlist is self-checking — no entry has rotted', () => {
    expect(staleOmissions(RUNTIME_EXPORTS, FACTORY_KEYS, BRIDGE_INTERNAL_OMISSIONS)).toStrictEqual([]);
  });
});

/**
 * MEMBERSHIP CRITERION: the file renders `<App />`, so the whole App component
 * tree — `initApp`, `engineStore.subscribeToEvents`, `createClaudeStore` — reads
 * the mocked namespace, and ANY gap degrades it silently. That, not "the file
 * mocks ../bridge", is what puts a file under the full-coverage contract. It is
 * approximated by a default import of App; a named-only import (such as
 * autoViewGenerator.test.ts's `{ computePaneGroups }`) renders nothing.
 *
 * Deliberately derived from the App import rather than from a glob for
 * `vi.mock('../bridge'`. Other test files carry bridge factories scoped to
 * their subject rather than to the whole App tree — correct for a focused unit
 * test: the engineStore harness consumers (engineStore.*.test.ts,
 * constraintVerdictParity.test.ts, engineStoreBridgeHandles.test.ts,
 * observedDemand.test.ts and selectiveDemand.test.ts) share the one
 * engineStore-scoped factory in engineStoreBridgeMockFactory.ts, and
 * sidecarPersistence.test.ts carries a deliberately narrow one.
 * viewport/Viewport.test.tsx uses a different specifier ('../../bridge').
 * Globbing would force them to full coverage or bloat the allowlist with 30+
 * meaningless entries, destroying its signal.
 *
 * This is the construction-time half of the enforcement. The use-time half is
 * harness.tsx's import-time check that the installed mocks are the harness's
 * own, so a file that renders <App /> only through renderAndWaitForReady, and
 * so never imports App (contextIntegration.test.tsx), is held by that check.
 */
describe('App harness membership', () => {
  const testsRoot = __dirname;
  const harnessModule = join(testsRoot, 'appHarness', 'harness');
  const DEFAULT_APP_IMPORT = /^import App\b[^;]*from '(\.\.\/)+App'/m;
  const DIRECT_APP_RENDER = /render\(\(\) => <App \/>\)/;

  function testFilesUnder(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) return testFilesUnder(path);
      return /\.test\.tsx?$/.test(entry.name) ? [path] : [];
    });
  }

  function harnessSpecifierFrom(file: string): string {
    const specifier = relative(dirname(file), harnessModule);
    return specifier.startsWith('.') ? specifier : `./${specifier}`;
  }

  const testFiles = testFilesUnder(testsRoot);
  const appRenderingFiles = testFiles.filter((file) => DEFAULT_APP_IMPORT.test(readFileSync(file, 'utf-8')));
  const relativeName = (file: string) => relative(testsRoot, file);

  it('selects every test file that renders <App /> directly, App.test.tsx among them', () => {
    const directRenderers = testFiles.filter((file) => DIRECT_APP_RENDER.test(readFileSync(file, 'utf-8')));
    const selected = appRenderingFiles.map(relativeName);
    expect(selected).toContain('App.test.tsx');
    expect(selected).toEqual(expect.arrayContaining(directRenderers.map(relativeName)));
  });

  it('every test file that renders <App /> installs the shared App harness', () => {
    const offenders = appRenderingFiles
      .filter((file) => !readFileSync(file, 'utf-8').includes(`from '${harnessSpecifierFrom(file)}'`))
      .map(relativeName);
    expect(
      offenders,
      'These files render <App /> without the shared App harness. Write these nine lines in each ' +
        '(specifiers as seen from gui/src/__tests__/), import from the harness, and prime it:\n  ' +
        Object.values(APP_HARNESS_INSTALL_LINES).join('\n  ') +
        '\n  beforeEach(primeAppHarness);',
    ).toStrictEqual([]);
  });
});
