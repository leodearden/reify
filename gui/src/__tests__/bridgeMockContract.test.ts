/**
 * Unit tests for the pure helpers in `bridgeMockContract` — the source-parsing
 * and set-difference logic behind the `bridgeMockCoverage` structural guard.
 *
 * Synthetic string/array input only: no vitest module mocks, no App render, no
 * filesystem. Two reasons this file exists rather than testing the guard alone:
 *
 *  1. Source-text parsing is the one fragile component of the guard. Keeping it
 *     a pure function over strings lets the nested-object, arrow-function,
 *     comment and same-line cases be pinned directly instead of hoped for.
 *  2. "The detector actually fires" is asserted MECHANICALLY here, by replaying
 *     the three historical defects it exists to prevent — `onModeShapeFrame`
 *     dropped from an App-shaped factory (task 6035), `onModeShapeFrame` /
 *     `subscribeToClaudeEvents` dropped from a contextIntegration-shaped
 *     factory (task 6039), and `syncDemand` dropped (task 6045) — rather than
 *     being claimed in a comment.
 */
import { describe, it, expect } from 'vitest';
import {
  extractBridgeFactoryKeys,
  extractBridgeMockOverrides,
  missingFactoryKeys,
  staleOmissions,
  unrestoredOverrides,
} from './bridgeMockContract';

describe('extractBridgeFactoryKeys', () => {
  it('returns the depth-1 keys of a plain vi.mock(\'../bridge\') factory', () => {
    const source = `
vi.mock('../bridge', () => ({
  getInitialState: vi.fn().mockResolvedValue(null),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
}));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual([
      'getInitialState',
      'onMeshUpdate',
    ]);
  });

  it('does NOT leak the keys of a nested object-literal value', () => {
    // The real App.test.tsx factory is full of `mockResolvedValue({ ... })`
    // entries whose inner keys (`file_path`, `meshes`, ...) are not bridge
    // exports. Capturing them would make the "every parsed key is a real
    // runtime export" sanity check fail — or worse, mask a real gap.
    const source = `
vi.mock('../bridge', () => ({
  getSourceLocation: vi.fn().mockResolvedValue({ file_path: '/test.ri', line: 1, deeper: { inner_key: 2 } }),
  focusEntity: vi.fn().mockResolvedValue(undefined),
}));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual([
      'getSourceLocation',
      'focusEntity',
    ]);
  });

  it('handles an arrow-function value whose body contains braces', () => {
    const source = `
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn().mockImplementation((cb) => { cb({ inner_key: 1 }); return () => {}; }),
  ask: vi.fn().mockResolvedValue(false),
}));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['onMeshUpdate', 'ask']);
  });

  it('ignores line comments and block comments between entries', () => {
    const source = `
vi.mock('../bridge', () => ({
  // saveFile: vi.fn(),   <- commented-out entry, must NOT count as covered
  getEntityTree: vi.fn().mockResolvedValue([]),
  /* block comment carrying a decoy: { fake_key: 1 } */
  syncDemand: vi.fn().mockResolvedValue(undefined),
}));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['getEntityTree', 'syncDemand']);
  });

  it('captures a key on the same line as the opening ({', () => {
    const source = `vi.mock('../bridge', () => ({ getInitialState: vi.fn(), ask: vi.fn() }));`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['getInitialState', 'ask']);
  });

  it('is not fooled by braces or colons inside string values', () => {
    const source = `
vi.mock('../bridge', () => ({
  getKernelStatus: vi.fn().mockResolvedValue({ message: 'not: a { key }' }),
  ask: vi.fn(),
}));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['getKernelStatus', 'ask']);
  });

  it('accepts a quoted key', () => {
    const source = `vi.mock('../bridge', () => ({ 'ask': vi.fn(), "cancelSolve": vi.fn() }));`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['ask', 'cancelSolve']);
  });

  it('reads ONLY the ../bridge factory, not neighbouring vi.mock calls', () => {
    // Both targets surround their bridge factory with other vi.mock calls
    // (../viewport, ../editor/Editor, ../debug, ...). Bleeding into those would
    // inflate the key set and make the guard vacuous.
    const source = `
vi.mock('../viewport', () => ({ Viewport: vi.fn(), DualViewport: vi.fn() }));
vi.mock('../bridge', () => ({
  getInitialState: vi.fn(),
}));
vi.mock('../debug', () => ({ initDebugBridge: vi.fn() }));
`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['getInitialState']);
  });

  it('preserves duplicates so the guard can detect a doubled entry', () => {
    const source = `vi.mock('../bridge', () => ({ ask: vi.fn(), ask: vi.fn() }));`;
    expect(extractBridgeFactoryKeys(source)).toStrictEqual(['ask', 'ask']);
  });

  it('returns [] rather than throwing when there is no ../bridge factory', () => {
    // A mis-targeted path must fail the guard's non-vacuity check loudly, not
    // blow up with an opaque parser error.
    expect(
      extractBridgeFactoryKeys(`vi.mock('../viewport', () => ({ Viewport: vi.fn() }));`),
    ).toStrictEqual([]);
    expect(extractBridgeFactoryKeys('')).toStrictEqual([]);
  });
});

/** An App.test.tsx-shaped export universe, trimmed to the names under test. */
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

describe('extractBridgeMockOverrides', () => {
  it('splits persistent assignments by whether a beforeEach re-establishes them', () => {
    const source = `
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(bridge.onMeshUpdate).mockResolvedValue(() => {});
});

it('a test', () => {
  vi.mocked(bridge.onMeshUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.saveFile).mockRejectedValue(new Error('disk full'));
});
`;
    expect(extractBridgeMockOverrides(source)).toStrictEqual({
      restored: ['onMeshUpdate'],
      overridden: ['onMeshUpdate', 'saveFile'],
    });
  });

  it('recognises the `(bridge as any).NAME` spelling both targets also use', () => {
    const source = `
beforeEach(() => {
  vi.mocked((bridge as any).onFileRemoved).mockResolvedValue(() => {});
});
it('a test', () => {
  vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([]);
});
`;
    expect(extractBridgeMockOverrides(source)).toStrictEqual({
      restored: ['onFileRemoved'],
      overridden: ['getMechanismDescriptors'],
    });
  });

  it('ignores *Once setters and mockClear — neither can leak', () => {
    const source = `
it('a test', () => {
  vi.mocked(bridge.openFile).mockResolvedValueOnce({ path: '', content: '' });
  vi.mocked(bridge.pickSavePath).mockRejectedValueOnce(new Error('x'));
  vi.mocked(bridge.setParameter).mockClear();
  expect(vi.mocked(bridge.saveFile).mock.calls).toHaveLength(0);
});
`;
    expect(extractBridgeMockOverrides(source)).toStrictEqual({ restored: [], overridden: [] });
  });

  it('counts a mock set inside a nested-brace body of a beforeEach as restored', () => {
    // Brace matching, not a line scan: an object literal or arrow body inside
    // the hook must not end the span early.
    const source = `
beforeEach(() => {
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], files: [] });
  vi.mocked(bridge.onMeshUpdate).mockImplementation(() => {
    return () => {};
  });
  vi.mocked(bridge.ask).mockResolvedValue(false);
});
`;
    expect(extractBridgeMockOverrides(source).restored).toStrictEqual([
      'ask',
      'getInitialState',
      'onMeshUpdate',
    ]);
    expect(extractBridgeMockOverrides(source).overridden).toStrictEqual([]);
  });

  it('handles several beforeEach blocks, including one inside a nested describe', () => {
    const source = `
beforeEach(() => {
  vi.mocked(bridge.getInitialState).mockResolvedValue(null);
});
describe('nested', () => {
  beforeEach(() => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '', content: '' });
  });
  it('a test', () => {
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/x', content: 'y' });
    vi.mocked(bridge.updateSource).mockResolvedValue(undefined);
  });
});
`;
    expect(extractBridgeMockOverrides(source)).toStrictEqual({
      restored: ['getInitialState', 'openFile'],
      overridden: ['openFile', 'updateSource'],
    });
  });

  it('returns empty sets for a source with no bridge mock assignments at all', () => {
    // The consumer asserts a floor on `restored` for exactly this reason: a
    // regex that stopped matching empties BOTH sets, which would otherwise make
    // the derived check pass vacuously.
    expect(extractBridgeMockOverrides('const x = 1;\n')).toStrictEqual({
      restored: [],
      overridden: [],
    });
  });
});

describe('unrestoredOverrides', () => {
  it('reports only the names no beforeEach restores, sorted', () => {
    expect(
      unrestoredOverrides({
        restored: ['onMeshUpdate', 'getInitialState'],
        overridden: ['updateSource', 'onMeshUpdate', 'claudeAbort'],
      }),
    ).toStrictEqual(['claudeAbort', 'updateSource']);
  });

  it('returns [] when every override has a restore', () => {
    expect(
      unrestoredOverrides({ restored: ['a', 'b'], overridden: ['b', 'a'] }),
    ).toStrictEqual([]);
  });

  it('fires for the leak shape the truth table names: factory key present, beforeEach missing', () => {
    // A test persistently swaps in a rejecting saveFile; nothing puts the
    // resolving default back, so every later test in the file runs against it.
    const source = `
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(bridge.getInitialState).mockResolvedValue(null);
});
it('save error path', () => {
  vi.mocked(bridge.saveFile).mockRejectedValue(new Error('disk full'));
});
`;
    expect(unrestoredOverrides(extractBridgeMockOverrides(source))).toStrictEqual(['saveFile']);
  });
});
