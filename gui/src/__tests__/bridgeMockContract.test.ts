/**
 * Unit tests for the pure helpers in `bridgeMockContract` — the set-difference
 * and harness-membership logic behind the `bridgeMockCoverage` structural
 * guard.
 *
 * Synthetic input only: no vitest module mocks, no App render, no
 * filesystem. "The detector actually fires" is asserted MECHANICALLY here, by
 * replaying the three historical defects it exists to prevent —
 * `onModeShapeFrame` dropped from App.test.tsx's then-inline factory
 * (task 6035), `onModeShapeFrame` / `subscribeToClaudeEvents` dropped from
 * contextIntegration.test.tsx's then-inline factory (task 6039), and
 * `syncDemand` dropped (task 6045) — rather than being claimed in a comment.
 */
import { describe, it, expect } from 'vitest';
import { harnessOffenders, missingFactoryKeys, staleOmissions } from './bridgeMockContract';

/** An export universe shaped like the shared App factory's, trimmed to the names under test. */
const APP_SHAPED_EXPORTS = [
  'getInitialState',
  'getEntityTree',
  'onMeshUpdate',
  'onModeShapeFrame',
  'syncDemand',
  'syncObservedDemand',
  'cancelSolve',
  'refreshFullState',
];

describe('missingFactoryKeys', () => {
  it('returns [] when the factory covers every runtime export', () => {
    expect(
      missingFactoryKeys(APP_SHAPED_EXPORTS, [...APP_SHAPED_EXPORTS], {}),
    ).toStrictEqual([]);
  });

  it('fires for the task-6035 defect: onModeShapeFrame dropped from an App-shaped factory', () => {
    const factoryKeys = APP_SHAPED_EXPORTS.filter((n) => n !== 'onModeShapeFrame');
    expect(missingFactoryKeys(APP_SHAPED_EXPORTS, factoryKeys, {})).toStrictEqual([
      'onModeShapeFrame',
    ]);
  });

  it('fires for the task-6045 defect: syncDemand dropped', () => {
    const factoryKeys = APP_SHAPED_EXPORTS.filter((n) => n !== 'syncDemand');
    expect(missingFactoryKeys(APP_SHAPED_EXPORTS, factoryKeys, {})).toStrictEqual([
      'syncDemand',
    ]);
  });

  it('fires for the task-6039 defect: a subscription export dropped from a smaller, contextIntegration-shaped factory', () => {
    // 6039's factory was a strict subset of App.test.tsx's; the gap was
    // onModeShapeFrame plus six more, of which subscribeToClaudeEvents is one.
    // A smaller factory must not dilute the detector.
    const contextExports = [
      'getInitialState',
      'getEntityTree',
      'onMeshUpdate',
      'onModeShapeFrame',
      'subscribeToClaudeEvents',
    ];
    const factoryKeys = contextExports.filter((n) => n !== 'subscribeToClaudeEvents');
    expect(missingFactoryKeys(contextExports, factoryKeys, {})).toStrictEqual([
      'subscribeToClaudeEvents',
    ]);
  });

  it('does not report an allowlisted omission', () => {
    const factoryKeys = APP_SHAPED_EXPORTS.filter((n) => n !== 'refreshFullState');
    expect(
      missingFactoryKeys(APP_SHAPED_EXPORTS, factoryKeys, {
        refreshFullState: 'test-only export; no non-test importer',
      }),
    ).toStrictEqual([]);
  });

  it('reports several gaps sorted, and reports a gap even when an unrelated allowlist entry exists', () => {
    const factoryKeys = APP_SHAPED_EXPORTS.filter(
      (n) => n !== 'syncDemand' && n !== 'cancelSolve' && n !== 'refreshFullState',
    );
    expect(
      missingFactoryKeys(APP_SHAPED_EXPORTS, factoryKeys, {
        refreshFullState: 'test-only export; no non-test importer',
      }),
    ).toStrictEqual(['cancelSolve', 'syncDemand']);
  });

  it('ignores factory keys that are not runtime exports (that is check (a)\'s job)', () => {
    expect(
      missingFactoryKeys(['ask'], ['ask', 'someKeyThatIsNotAnExport'], {}),
    ).toStrictEqual([]);
  });
});

describe('staleOmissions', () => {
  const runtimeExports = ['ask', 'refreshFullState', 'lspRequest'];

  it('returns [] while every allowlist entry still exhibits its asymmetry', () => {
    expect(
      staleOmissions(runtimeExports, ['ask'], {
        refreshFullState: 'test-only export',
        lspRequest: 'no non-test importer',
      }),
    ).toStrictEqual([]);
  });

  it('reports an allowlist entry bridge.ts no longer exports', () => {
    expect(
      staleOmissions(runtimeExports, ['ask'], {
        refreshFullState: 'test-only export',
        deletedExport: 'stale — bridge.ts dropped this name',
      }),
    ).toStrictEqual(['deletedExport']);
  });

  it('reports an allowlist entry that is in fact mocked by the factory', () => {
    // Allowlisting a name that IS covered turns the allowlist into a rubber
    // stamp: the reason stops being true and nobody notices.
    expect(
      staleOmissions(runtimeExports, ['ask', 'refreshFullState'], {
        refreshFullState: 'test-only export',
      }),
    ).toStrictEqual(['refreshFullState']);
  });

  it('returns the sorted union of both staleness conditions, without duplicates', () => {
    expect(
      staleOmissions(runtimeExports, ['ask', 'refreshFullState'], {
        refreshFullState: 'covered after all',
        deletedExport: 'no longer exported',
      }),
    ).toStrictEqual(['deletedExport', 'refreshFullState']);
  });

  it('returns [] for an empty allowlist', () => {
    expect(staleOmissions(runtimeExports, ['ask'], {})).toStrictEqual([]);
  });
});

describe('harnessOffenders', () => {
  const harnessModule = '/gui/src/__tests__/appHarness/harness';
  const appImport = "import App from '../App';\n";

  it('reports a file that default-imports App without importing the harness', () => {
    const files = [{ path: '/gui/src/__tests__/App.x.test.tsx', source: appImport }];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual(['/gui/src/__tests__/App.x.test.tsx']);
  });

  it('ignores a file whose App import is named-only, since it renders nothing', () => {
    const files = [{ path: '/gui/src/__tests__/autoView.test.ts', source: "import { computePaneGroups } from '../App';\n" }];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([]);
  });

  it('accepts a bare side-effect harness import, alone or beside a named one, resolved from the file\'s own directory', () => {
    const files = [
      { path: '/gui/src/__tests__/App.bare.test.tsx', source: `${appImport}import './appHarness/harness';\n` },
      { path: '/gui/src/__tests__/App.both.test.tsx', source: `${appImport}import './appHarness/harness';\nimport { renderAndWaitForReady, oneFileState } from './appHarness/harness';\n` },
      { path: '/gui/src/__tests__/nested/App.deep.test.tsx', source: "import App from '../../App';\nimport '../appHarness/harness';\n" },
      { path: '/gui/src/__tests__/helperOnly.test.tsx', source: "import './appHarness/harness';\nimport { renderAndWaitForReady } from './appHarness/harness';\n" },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([]);
  });

  it('reports a named-only harness import, which the TS transform drops once no binding has a value use', () => {
    const files = [
      { path: '/gui/src/__tests__/App.named.test.tsx', source: `${appImport}import { renderAndWaitForReady } from './appHarness/harness';\n` },
      { path: '/gui/src/__tests__/App.multiline.test.tsx', source: `${appImport}import {\n  renderAndWaitForReady,\n  oneFileState,\n} from './appHarness/harness';\n` },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([
      '/gui/src/__tests__/App.multiline.test.tsx',
      '/gui/src/__tests__/App.named.test.tsx',
    ]);
  });

  it('reports a type-only or a commented-out harness import', () => {
    const files = [
      { path: '/gui/src/__tests__/App.typeOnly.test.tsx', source: `${appImport}import type { PrimedModule } from './appHarness/harness';\n` },
      { path: '/gui/src/__tests__/App.commented.test.tsx', source: `${appImport}// import './appHarness/harness';\n` },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([
      '/gui/src/__tests__/App.commented.test.tsx',
      '/gui/src/__tests__/App.typeOnly.test.tsx',
    ]);
  });

  it('reports a file that imports a harness helper without App when it lacks the bare import (the contextIntegration / App.panes shape)', () => {
    const files = [
      { path: '/gui/src/__tests__/contextShaped.test.tsx', source: "import { renderAndWaitForReady } from './appHarness/harness';\n" },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual(['/gui/src/__tests__/contextShaped.test.tsx']);
  });

  it('selects a harness import written with double quotes or without a semicolon, yet accepts only the exact bare line', () => {
    const files = [
      { path: '/gui/src/__tests__/doubleQuoted.test.tsx', source: 'import { renderAndWaitForReady } from "./appHarness/harness";\n' },
      { path: '/gui/src/__tests__/noSemicolon.test.tsx', source: "import { renderAndWaitForReady } from './appHarness/harness'\n" },
      { path: '/gui/src/__tests__/bareDoubleQuoted.test.tsx', source: 'import "./appHarness/harness";\nimport { oneFileState } from "./appHarness/harness";\n' },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([
      '/gui/src/__tests__/bareDoubleQuoted.test.tsx',
      '/gui/src/__tests__/doubleQuoted.test.tsx',
      '/gui/src/__tests__/noSemicolon.test.tsx',
    ]);
  });

  it('reports a nested file whose harness specifier does not resolve from its directory, sorted with the rest', () => {
    const files = [
      { path: '/gui/src/__tests__/nested/App.deep.test.tsx', source: "import App from '../../App';\nimport './appHarness/harness';\n" },
      { path: '/gui/src/__tests__/App.a.test.tsx', source: appImport },
    ];
    expect(harnessOffenders(files, harnessModule)).toStrictEqual([
      '/gui/src/__tests__/App.a.test.tsx',
      '/gui/src/__tests__/nested/App.deep.test.tsx',
    ]);
  });
});
