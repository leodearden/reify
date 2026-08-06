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

/**
 * Runtime exports a factory may legitimately omit, each with the reason it is
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
 *     to both factories when the export is function-shaped: three entries that
 *     rested on a premise about ../viewport being mocked were retired that way,
 *     because one line in each factory beats a premise the guard cannot check.
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

/**
 * Files subject to the full-coverage contract.
 *
 * MEMBERSHIP CRITERION: this file renders `<App />`, so the whole App component
 * tree — `initApp`, `engineStore.subscribeToEvents`, `createClaudeStore` — reads
 * the mocked namespace, and ANY gap degrades it silently. That, not "the file
 * mocks ../bridge", is the test for adding a row here.
 *
 * Deliberately an explicit table rather than a glob for `vi.mock('../bridge'`.
 * Four other test files (engineStore.test.ts, observedDemand.test.ts,
 * selectiveDemand.test.ts, sidecarPersistence.test.ts) carry deliberately narrow,
 * subject-scoped bridge factories — correct for a focused unit test — and
 * viewport/Viewport.test.tsx uses a different specifier ('../../bridge').
 * Globbing would force them to full coverage or bloat the allowlist with 30+
 * meaningless entries, destroying its signal.
 *
 * `omissions` is per-row, not global, because check (c) fails an entry that the
 * factory DOES mock: a single shared table therefore forces every target to
 * carry the same key set. Both rows point at the same table today because both
 * factories are complete in the same way; a future target with a legitimately
 * narrower factory gets its own table rather than making this one lie.
 *
 * `minFactoryKeys` is a per-target non-vacuity floor. It does NOT protect check
 * (b) — under-parsing makes (b) report MORE gaps, not fewer, so (b) already
 * fails loudly on any under-capture. What the floor buys is a clearer failure
 * for the one case (b) words badly: a mis-targeted or renamed path, where the
 * parser finds no factory at all and (b) would otherwise dump the entire
 * 68-name export list as "missing".
 */
const TARGETS = [
  { file: 'App.test.tsx', minFactoryKeys: 55, omissions: BRIDGE_INTERNAL_OMISSIONS },
  { file: 'contextIntegration.test.tsx', minFactoryKeys: 55, omissions: BRIDGE_INTERNAL_OMISSIONS },
] as const;

describe.each(TARGETS)(
  'bridge-mock coverage: bridge.ts runtime exports ↔ $file vi.mock factory',
  ({ file, minFactoryKeys, omissions }) => {
    const factoryKeys = extractBridgeFactoryKeys(readFileSync(join(__dirname, file), 'utf-8'));

    it('(a) extraction sanity — neither side is vacuously empty or over-captured', () => {
      // Without this, a parser that silently returned [] would make (b) pass.
      expect(RUNTIME_EXPORTS.length).toBeGreaterThanOrEqual(60);
      for (const name of [
        'getInitialState',
        'onModeShapeFrame',
        'syncDemand',
        'claudePermissionDecision',
      ]) {
        expect(RUNTIME_EXPORTS, `bridge.ts must export '${name}'`).toContain(name);
      }

      expect(factoryKeys.length).toBeGreaterThanOrEqual(minFactoryKeys);
      expect(factoryKeys).toContain('getInitialState');
      expect(factoryKeys).toContain('onMeshUpdate');
      // A doubled factory entry is a merge artefact, not a covered export.
      expect(new Set(factoryKeys).size).toBe(factoryKeys.length);
      // Catches a parser that leaked the inner keys of a nested object literal
      // (`file_path`, `meshes`, ...) — and a factory key that is no longer an
      // export at all.
      const notExports = factoryKeys.filter((k) => !RUNTIME_EXPORTS.includes(k));
      expect(notExports).toStrictEqual([]);
    });

    it('(b) every bridge.ts runtime export is mocked or documented as omitted', () => {
      expect(missingFactoryKeys(RUNTIME_EXPORTS, factoryKeys, omissions)).toStrictEqual([]);
    });

    it('(c) the allowlist is self-checking — no entry has rotted', () => {
      expect(staleOmissions(RUNTIME_EXPORTS, factoryKeys, omissions)).toStrictEqual([]);
    });
  },
);
