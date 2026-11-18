/**
 * Unit tests for the pure set helpers in `bridgeMockContract` — the
 * set-difference logic behind the `bridgeMockCoverage` structural guard.
 *
 * Synthetic array input only: no vitest module mocks, no App render, no
 * filesystem. "The detector actually fires" is asserted MECHANICALLY here, by
 * replaying the three historical defects it exists to prevent —
 * `onModeShapeFrame` dropped from App.test.tsx's then-inline factory
 * (task 6035), `onModeShapeFrame` / `subscribeToClaudeEvents` dropped from
 * contextIntegration.test.tsx's then-inline factory (task 6039), and
 * `syncDemand` dropped (task 6045) — rather than being claimed in a comment.
 */
import { describe, it, expect } from 'vitest';
import { missingFactoryKeys, staleOmissions } from './bridgeMockContract';

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
