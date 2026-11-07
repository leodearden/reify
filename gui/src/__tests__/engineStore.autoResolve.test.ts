import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState } from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnMeshUpdate,
  mockOnValueUpdate,
  mockOnConstraintUpdate,
  mockOnEvaluationStatus,
  mockOnMeshRemoved,
  mockOnValueRemoved,
  mockOnConstraintRemoved,
  mockOnAutoResolveStart,
  mockOnAutoResolveIteration,
  mockOnAutoResolveComplete,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { sampleIteration } from './engineStoreFixtures';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

describe('engineStore autoResolve loop state', () => {
  it('(a) initial state.autoResolve equals { active: false, iterations: [] }', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.autoResolve).toEqual({ active: false, iterations: [] });
      dispose();
    });
  });

  it('(b) beginAutoResolveLoop sets active=true; the new loop\'s first iteration is what drops the old samples', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.iterations).toHaveLength(1);

      beginAutoResolveLoop();
      expect(state.autoResolve.active).toBe(true);
      // The clear is deferred, not skipped: the previous samples stay visible
      // until this loop actually has something to show.
      expect(state.autoResolve.iterations).toHaveLength(1);

      applyAutoResolveIteration({ ...sampleIteration, iteration: 9 });
      // A new loop must not show the previous loop's samples.
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].iteration).toBe(9);
      dispose();
    });
  });

  it('(c) applyAutoResolveIteration appends to iterations', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0]).toEqual(sampleIteration);

      const iter2 = { ...sampleIteration, iteration: 2, driving_metric_value: 165 };
      applyAutoResolveIteration(iter2);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.iterations[1].iteration).toBe(2);
      dispose();
    });
  });

  it('(d) endAutoResolveLoop sets active=false and PRESERVES iterations', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2 });
      expect(state.autoResolve.iterations).toHaveLength(2);

      endAutoResolveLoop();
      expect(state.autoResolve.active).toBe(false);
      expect(state.autoResolve.iterations).toHaveLength(2);
      dispose();
    });
  });

  it('(e) endAutoResolveLoop preserves the completed loop\'s samples and canonical metric', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2 });

      endAutoResolveLoop();
      // The loop is finished — but a finished loop must stay READABLE. The panel
      // mounts on `iterations.length > 0`, not on `active`, so clearing here would
      // make the result of the loop the user just ran vanish the instant it lands.
      expect(state.autoResolve.active).toBe(false);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.iterations[1].iteration).toBe(2);
      // The canonical must survive too, or the chart's y-axis label goes blank
      // on a completed loop.
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      dispose();
    });
  });
});

describe('engineStore autoResolve deferred loop reset', () => {
  it('(1) beginAutoResolveLoop does NOT open an empty iterations window', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      // A completed loop holding one sample.
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      endAutoResolveLoop();
      expect(state.autoResolve.iterations).toHaveLength(1);

      // The engine re-fires the whole start/iteration/complete trio on EVERY
      // re-eval. If `begin` cleared eagerly the store would walk
      // [iter] -> [] -> [iter] and the data-gated panel would unmount, resize
      // the side-panel grid and lose scroll position on every commit.
      beginAutoResolveLoop();
      expect(state.autoResolve.active).toBe(true);
      expect(state.autoResolve.iterations).toHaveLength(1);
      dispose();
    });
  });

  it('(2) the first accepted iteration of the new loop REPLACES the previous samples', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      endAutoResolveLoop();

      beginAutoResolveLoop();
      applyAutoResolveIteration({ ...sampleIteration, iteration: 7 });
      // Deferred clear ran, THEN the push — it must not append to length 2.
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].iteration).toBe(7);
      dispose();
    });
  });

  it('(3) a new loop with a different driving_metric is accepted, not dropped against the stale canonical', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      endAutoResolveLoop();
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');

      beginAutoResolveLoop();
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: 'mass', driving_metric_value: 3.1 });

      // The deferred clear MUST run before the canonical-mismatch check. If it
      // did not, every iteration of the new loop would be dropped against the
      // previous loop's canonical and the panel would freeze on the old loop.
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('mass');
      const mismatchWarns = warnSpy.mock.calls.filter(
        ([msg]) => typeof msg === 'string' && msg.includes('driving_metric mismatch'),
      );
      expect(mismatchWarns).toHaveLength(0);
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(4) a loop that produces no iteration at all clears the previous loop on end', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      endAutoResolveLoop();
      expect(state.autoResolve.iterations).toHaveLength(1);

      // start -> complete with nothing in between: the deferred clear was never
      // discharged by an iteration, so `end` must discharge it. Otherwise the
      // previous loop's samples would be stranded on screen indefinitely.
      beginAutoResolveLoop();
      endAutoResolveLoop();
      expect(state.autoResolve.active).toBe(false);
      expect(state.autoResolve.iterations).toHaveLength(0);
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();
      dispose();
    });
  });
});

describe('engineStore autoResolve initFromState reset', () => {
  it('initFromState drops the previous file\'s completed auto-resolve loop', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop, initFromState } = createEngineStore();

      // File A runs a loop to completion; its samples now persist (they are what
      // the data-gated panel renders).
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      endAutoResolveLoop();
      expect(state.autoResolve.iterations).toHaveLength(1);

      // Opening file B must not leave file A's resolved parameters and
      // constraint rows mounted — that is actively misleading.
      const guiState: GuiState = {
        fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: [],
      };
      initFromState(guiState);

      expect(state.autoResolve.active).toBe(false);
      expect(state.autoResolve.iterations).toHaveLength(0);
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();
      expect(state.autoResolve.pendingReset).toBeFalsy();
      dispose();
    });
  });
});

describe('engineStore autoResolve subscribeToEvents wiring', () => {
  it('(a) subscribeToEvents calls onAutoResolveStart, onAutoResolveIteration, onAutoResolveComplete', async () => {
    await createRoot(async (dispose) => {
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());

      const store = createEngineStore();
      await store.subscribeToEvents();

      expect(mockOnAutoResolveStart).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnAutoResolveIteration).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnAutoResolveComplete).toHaveBeenCalledWith(expect.any(Function));
      dispose();
    });
  });

  it('(b) auto-resolve-start callback flips active=true and defers the clear', async () => {
    await createRoot(async (dispose) => {
      let startCb: (() => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockImplementation(async (cb) => { startCb = cb; return vi.fn(); });

      const store = createEngineStore();
      store.applyAutoResolveIteration(sampleIteration);
      await store.subscribeToEvents();

      startCb!();
      expect(store.state.autoResolve.active).toBe(true);
      // Deferred: the previous loop's sample survives until this loop emits one.
      expect(store.state.autoResolve.iterations).toHaveLength(1);
      dispose();
    });
  });

  it('(c) auto-resolve-iteration callback appends to iterations', async () => {
    await createRoot(async (dispose) => {
      let iterCb: ((iter: typeof sampleIteration) => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveIteration.mockImplementation(async (cb) => { iterCb = cb as typeof iterCb; return vi.fn(); });

      const store = createEngineStore();
      await store.subscribeToEvents();

      iterCb!(sampleIteration);
      expect(store.state.autoResolve.iterations).toHaveLength(1);
      expect(store.state.autoResolve.iterations[0]).toEqual(sampleIteration);
      dispose();
    });
  });

  it('(d) auto-resolve-complete callback sets active=false and preserves iterations', async () => {
    await createRoot(async (dispose) => {
      let startCb: (() => void) | undefined;
      let iterCb: ((iter: typeof sampleIteration) => void) | undefined;
      let completeCb: (() => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockImplementation(async (cb) => { startCb = cb; return vi.fn(); });
      mockOnAutoResolveIteration.mockImplementation(async (cb) => { iterCb = cb as typeof iterCb; return vi.fn(); });
      mockOnAutoResolveComplete.mockImplementation(async (cb) => { completeCb = cb; return vi.fn(); });

      const store = createEngineStore();
      await store.subscribeToEvents();

      startCb!();
      iterCb!(sampleIteration);
      completeCb!();

      expect(store.state.autoResolve.active).toBe(false);
      // A completed loop stays readable — the panel is data-gated, not active-gated.
      expect(store.state.autoResolve.iterations).toHaveLength(1);
      dispose();
    });
  });

  it('(e) cleanup() invokes the three new unlisten fns', async () => {
    await createRoot(async (dispose) => {
      const unlistenStart = vi.fn();
      const unlistenIteration = vi.fn();
      const unlistenComplete = vi.fn();
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockResolvedValue(unlistenStart);
      mockOnAutoResolveIteration.mockResolvedValue(unlistenIteration);
      mockOnAutoResolveComplete.mockResolvedValue(unlistenComplete);

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();

      cleanup();
      expect(unlistenStart).toHaveBeenCalled();
      expect(unlistenIteration).toHaveBeenCalled();
      expect(unlistenComplete).toHaveBeenCalled();
      dispose();
    });
  });
});

describe('engineStore autoResolve driving_metric invariance', () => {
  it('(1) iteration with matching driving_metric is appended', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      const iter2 = { ...sampleIteration, iteration: 2, driving_metric_value: 165 };
      applyAutoResolveIteration(iter2);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.iterations[1]).toEqual(iter2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      dispose();
    });
  });

  it('(2) iteration with mismatched driving_metric is dropped', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      const mismatchedIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('max_von_mises');
      dispose();
    });
  });

  it('(3) iteration without driving_metric is appended regardless', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      // Apply an iteration without driving_metric
      const { driving_metric, driving_metric_value, ...noMetricIter } = sampleIteration;
      applyAutoResolveIteration({ ...noMetricIter, iteration: 2 });
      expect(state.autoResolve.iterations).toHaveLength(2);
      // Canonical metric is preserved: subsequent mismatched iteration is still rejected
      const mismatchedIter = { ...sampleIteration, iteration: 3, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      dispose();
    });
  });

  it('(4) first iteration with driving_metric establishes the canonical; no-canonical state accepts any metric', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      // Initially no canonical — the loop accepts ANY driving_metric value.
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // First iteration has no driving_metric — accepted; canonical remains undefined.
      const { driving_metric, driving_metric_value, ...noMetricIter } = sampleIteration;
      applyAutoResolveIteration({ ...noMetricIter, iteration: 1 });
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // Second iteration declares driving_metric='displacement' — accepted (no canonical
      // conflict) and ESTABLISHES the canonical for the remainder of the loop.
      const displacementIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 0.5 };
      applyAutoResolveIteration(displacementIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      // Third iteration with a different metric — DROPPED (canonical is now 'displacement').
      const mismatchedIter = { ...sampleIteration, iteration: 3, driving_metric_value: 190 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      dispose();
    });
  });

  it('(5) console.warn fires on rejection', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const mismatchedIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('driving_metric mismatch'),
        expect.objectContaining({
          iteration: 2,
          canonical: 'max_von_mises',
          received: 'displacement',
        }),
      );
      expect(state.autoResolve.iterations).toHaveLength(1);
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(6) endAutoResolveLoop + beginAutoResolveLoop resets canonical so a new loop adopts a different driving_metric', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      const iterA = { ...sampleIteration, driving_metric: 'A', driving_metric_value: 1 };
      applyAutoResolveIteration(iterA);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');
      expect(state.autoResolve.iterations).toHaveLength(1);

      endAutoResolveLoop();
      beginAutoResolveLoop();
      // The reset is deferred to the new loop's first iteration, so the stale
      // canonical is still readable here — what matters is that it does not
      // survive that iteration, nor cause it to be dropped as a mismatch.
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');

      const iterB = { ...sampleIteration, driving_metric: 'B', driving_metric_value: 1 };
      applyAutoResolveIteration(iterB);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('B');
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('B');
      dispose();
    });
  });

  it('(7) a second beginAutoResolveLoop without endAutoResolveLoop in between still resets canonical on its first iteration', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      const iterA = { ...sampleIteration, driving_metric: 'A', driving_metric_value: 1 };
      applyAutoResolveIteration(iterA);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');

      // Fresh begin without an explicit end — the reset is owed, not yet done.
      beginAutoResolveLoop();
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');
      expect(state.autoResolve.iterations).toHaveLength(1);

      const iterB = { ...sampleIteration, driving_metric: 'B', driving_metric_value: 1 };
      applyAutoResolveIteration(iterB);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('B');
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('B');
      dispose();
    });
  });
});

describe('engineStore autoResolve empty-string driving_metric', () => {
  it('(a) empty-string driving_metric after canonical established: appended, canonical unchanged, dedicated warn fires', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      // Iteration is still appended (not dropped)
      expect(state.autoResolve.iterations).toHaveLength(2);
      // Canonical is unchanged
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      // Dedicated empty-string warn fires (not the mismatch warn)
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('empty driving_metric'),
        expect.objectContaining({ iteration: 2 }),
      );
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(b) empty-string driving_metric as first iteration: appended, canonical stays undefined, warn fires', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      // Iteration is appended
      expect(state.autoResolve.iterations).toHaveLength(1);
      // canonical must NOT be set to ''
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();
      // empty-string warn fires
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('empty driving_metric'),
        expect.objectContaining({ iteration: 1 }),
      );
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(c) real-metric iteration following empty-string iteration still establishes canonical', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      // First: empty-string driving_metric — canonical stays undefined
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // Second: real driving_metric — must be appended and establish the canonical
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 0.8 });
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      const emptyWarnCalls = warnSpy.mock.calls.filter(
        ([msg]) => typeof msg === 'string' && msg.includes('empty driving_metric'),
      );
      expect(emptyWarnCalls).toHaveLength(1);
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(d) warn fires at most once per loop even when multiple empty-string iterations arrive', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      // Three consecutive empty-string iterations
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 3, driving_metric: '' });

      expect(state.autoResolve.iterations).toHaveLength(3);
      // Warn fires exactly once, not three times
      const emptyWarnCalls = warnSpy.mock.calls.filter(([msg]) =>
        typeof msg === 'string' && msg.includes('empty driving_metric'),
      );
      expect(emptyWarnCalls).toHaveLength(1);

      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(e) beginAutoResolveLoop resets warn-once flag so a new loop can warn again', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();

      // First loop: empty-string → warn fires once and flag is set
      beginAutoResolveLoop();
      const spy1 = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      expect(spy1.mock.calls.filter(([m]) => typeof m === 'string' && m.includes('empty driving_metric'))).toHaveLength(1);
      spy1.mockRestore();

      // Second loop: beginAutoResolveLoop clears the flag — warn fires again on the first empty-string
      beginAutoResolveLoop();
      const spy2 = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      expect(spy2.mock.calls.filter(([m]) => typeof m === 'string' && m.includes('empty driving_metric'))).toHaveLength(1);
      spy2.mockRestore();
      dispose();
    });
  });
});
