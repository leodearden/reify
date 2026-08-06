/**
 * Structural coverage guard (task-6053):
 * runtime exports of bridge.ts ↔ keys of a target test file's
 * `vi.mock('../bridge', () => ({ ... }))` factory.
 *
 * Invariant: every runtime export of `../bridge` is either mocked by the target
 * factory or carries a documented entry in `DELIBERATE_OMISSIONS`. The factory
 * is a NON-partial mock, so an omitted export makes vitest throw
 * `No "X" export is defined on the "../bridge" mock` SYNCHRONOUSLY at property
 * access — and `initApp`'s run of sibling try/catch blocks makes that same
 * defect surface with opposite signatures: an export consumed by
 * `engineStore.subscribeToEvents` yields a DOM toast and ZERO stderr, while one
 * consumed directly by an `initApp` block yields an stderr flood with every
 * test still green. Tasks 6035, 6039 and 6045 each fixed one name of this class
 * by hand, one export at a time; this guard detects the whole class.
 *
 * Shape mirrors `debugParity.test.ts` — the house pattern for "two artifacts
 * must stay in lockstep, with documented legitimate asymmetries": mock the
 * runtime deps, read one side authoritatively, parse the other, keep a named
 * allowlist carrying prose reasons, and add both an extraction-sanity check and
 * an allowlist-self-check so neither side can rot into a rubber stamp.
 */
import { describe, it, expect, vi } from 'vitest';

// These three mocks make bridge.ts importable at collection time without a
// Tauri runtime. Proven sufficient: bridge.test.ts imports the same module with
// exactly this set. `@tauri-apps/plugin-dialog` is the reason this guard cannot
// live inside App.test.tsx — that file does not mock it.
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

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// REAL module (the three mocks above only stub its Tauri dependencies).
import * as bridge from '../bridge';

import { extractBridgeFactoryKeys, missingFactoryKeys, staleOmissions } from './bridgeMockContract';

/**
 * Authoritative export list. `Object.keys` on the real ESM namespace needs no
 * regex and cannot drift: TS types erase, so this is exactly the set of runtime
 * exports a `vi.mock` factory has to satisfy. Do NOT replace with a source
 * parse of bridge.ts — the factory side is parsed only because a `vi.mock`
 * factory is introspectable solely from the file that installs it.
 */
const RUNTIME_EXPORTS = Object.keys(bridge);

const APP_TEST_SOURCE = readFileSync(join(__dirname, 'App.test.tsx'), 'utf-8');
const appFactoryKeys = extractBridgeFactoryKeys(APP_TEST_SOURCE);

/**
 * Runtime exports a target factory may legitimately omit, each with the reason
 * it is unreachable from a rendered `<App />`. Check (c) keeps every entry
 * honest: allowlisting a name that bridge.ts no longer exports, or that the
 * factory in fact mocks, fails.
 *
 * The bar for adding an entry is "no non-test importer can reach it through the
 * mocked namespace", NOT "adding the key is inconvenient". `claudePermissionDecision`
 * is deliberately NOT here: App.tsx imports it and calls it in `createClaudeStore`'s
 * `onPermissionDecision` handler, so it is genuinely reachable.
 */
const DELIBERATE_OMISSIONS: Record<string, string> = {
  BUILD_CONTEXT_HANDLED_FIELDS:
    'const array; only importers are __tests__/types.typecheck.ts and __tests__/claudeBridge.test.ts (the ChatPanel.tsx hit is a comment)',
  MESSAGE_CONTEXT_FIELD_MAP:
    'const map iterated by mapContextToWire inside bridge.ts; no non-test importer (the ChatPanel.tsx hit is a comment)',
  lspRequest:
    'no non-test importer at all — editor/lspClient.ts declares its OWN local lspRequest and calls invoke directly',
  mapContextToWire:
    'called only by claudeSendMessage within bridge.ts; a same-module call never goes through the mocked namespace',
  onDiagnostics: 'zero references repo-wide',
  onFeaCaseChanged: 'called only by subscribeFeaCaseToStore within bridge.ts',
  refreshFullState: 'test-only — sole importer is __tests__/bridge.test.ts',
  setActiveFeaCase:
    'sole importer is viewport/Viewport.tsx, reachable only through the ../viewport barrel which every target mocks wholesale — unmocking ../viewport flips this to reachable',
  subscribeFeaCaseToStore:
    'sole importer is viewport/Viewport.tsx, reachable only through the ../viewport barrel which every target mocks wholesale — unmocking ../viewport flips this to reachable',
  validatePayload: "called only from bridge.ts's own claude/sidecar listeners",
};

describe('bridge-mock coverage: bridge.ts runtime exports ↔ App.test.tsx vi.mock factory', () => {
  it('(a) extraction sanity — neither side is vacuously empty or over-captured', () => {
    // Without this, a parser that silently returned [] would make (b) pass.
    expect(RUNTIME_EXPORTS.length).toBeGreaterThanOrEqual(60);
    for (const name of ['getInitialState', 'onModeShapeFrame', 'syncDemand', 'claudePermissionDecision']) {
      expect(RUNTIME_EXPORTS, `bridge.ts must export '${name}'`).toContain(name);
    }

    expect(appFactoryKeys.length).toBeGreaterThanOrEqual(50);
    expect(appFactoryKeys).toContain('getInitialState');
    expect(appFactoryKeys).toContain('onMeshUpdate');
    // A doubled factory entry is a merge artefact, not a covered export.
    expect(new Set(appFactoryKeys).size).toBe(appFactoryKeys.length);
    // Catches a parser that leaked the inner keys of a nested object literal
    // (`file_path`, `meshes`, ...) — and a factory key that is no longer an
    // export at all.
    const notExports = appFactoryKeys.filter((k) => !RUNTIME_EXPORTS.includes(k));
    expect(notExports).toStrictEqual([]);
  });

  it('(b) every bridge.ts runtime export is mocked or documented as omitted', () => {
    expect(missingFactoryKeys(RUNTIME_EXPORTS, appFactoryKeys, DELIBERATE_OMISSIONS)).toStrictEqual(
      [],
    );
  });

  it('(c) the allowlist is self-checking — no entry has rotted', () => {
    expect(staleOmissions(RUNTIME_EXPORTS, appFactoryKeys, DELIBERATE_OMISSIONS)).toStrictEqual([]);
  });
});
