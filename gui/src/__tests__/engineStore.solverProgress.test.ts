import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnMeshUpdate,
  mockOnValueUpdate,
  mockOnConstraintUpdate,
  mockOnEvaluationStatus,
  mockOnMeshRemoved,
  mockOnValueRemoved,
  mockOnConstraintRemoved,
  mockOnSolverProgress,
  mockCancelSolve,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

describe('engineStore solverProgress', () => {
  it('(step-1a) initial state.solverProgress is { latest: null, trace: [], visible: false, coarseReached: false }', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.solverProgress).toEqual({ latest: null, trace: [], visible: false, coarseReached: false });
      dispose();
    });
  });

  it('(step-1b) applySolverProgress sets latest and appends to trace', () => {
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      const tick1 = { solver_kind: 'cg' as const, iter: 1, residual: 0.5 };
      applySolverProgress(tick1);
      expect(state.solverProgress.latest).toEqual(tick1);
      expect(state.solverProgress.trace).toHaveLength(1);
      expect(state.solverProgress.trace[0]).toEqual(tick1);

      const tick2 = { solver_kind: 'cg' as const, iter: 2, residual: 0.3 };
      applySolverProgress(tick2);
      expect(state.solverProgress.latest).toEqual(tick2);
      expect(state.solverProgress.trace).toHaveLength(2);
      dispose();
    });
  });

  it('(step-1c) visible stays false before debounce timer fires', () => {
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      expect(state.solverProgress.visible).toBe(false);
      dispose();
    });
  });

  it('(step-3a) visible becomes true after >1s debounce', () => {
    vi.useFakeTimers();
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      expect(state.solverProgress.visible).toBe(false);
      vi.advanceTimersByTime(1000);
      expect(state.solverProgress.visible).toBe(true);
      dispose();
    });
    vi.useRealTimers();
  });

  it('(step-3b) subsequent ticks keep visible true once debounce fires', () => {
    vi.useFakeTimers();
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      vi.advanceTimersByTime(1000);
      expect(state.solverProgress.visible).toBe(true);
      applySolverProgress({ solver_kind: 'cg' as const, iter: 2, residual: 0.3 });
      applySolverProgress({ solver_kind: 'cg' as const, iter: 3, residual: 0.1 });
      expect(state.solverProgress.visible).toBe(true);
      dispose();
    });
    vi.useRealTimers();
  });

  it('(step-5) coarseReached flips true on first residual < 1e-2 and stays true', () => {
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      expect(state.solverProgress.coarseReached).toBe(false);
      applySolverProgress({ solver_kind: 'cg' as const, iter: 2, residual: 0.05 });
      expect(state.solverProgress.coarseReached).toBe(false);
      // First tick below 1e-2
      applySolverProgress({ solver_kind: 'cg' as const, iter: 3, residual: 9e-3 });
      expect(state.solverProgress.coarseReached).toBe(true);
      // Stays true even if a later residual is above threshold (defensive)
      applySolverProgress({ solver_kind: 'cg' as const, iter: 4, residual: 0.1 });
      expect(state.solverProgress.coarseReached).toBe(true);
      dispose();
    });
  });

  it('(step-7a) setEvalStatus({phase:idle}) resets solverProgress to initial state', () => {
    vi.useFakeTimers();
    createRoot((dispose) => {
      const { state, applySolverProgress, setEvalStatus } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      vi.advanceTimersByTime(1000);
      expect(state.solverProgress.visible).toBe(true);
      expect(state.solverProgress.trace).toHaveLength(1);

      setEvalStatus({ phase: 'idle' });
      expect(state.solverProgress).toEqual({ latest: null, trace: [], visible: false, coarseReached: false });
      dispose();
    });
    vi.useRealTimers();
  });

  it('(step-7b) fast-solve: timer cleared when idle arrives before 1s, visible stays false', () => {
    vi.useFakeTimers();
    createRoot((dispose) => {
      const { state, applySolverProgress, setEvalStatus } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      expect(state.solverProgress.visible).toBe(false);
      setEvalStatus({ phase: 'idle' });
      vi.advanceTimersByTime(1000);
      // Timer must have been cleared — visible stays false after reset
      expect(state.solverProgress.visible).toBe(false);
      dispose();
    });
    vi.useRealTimers();
  });

  it('(step-7c) resetSolverProgress() directly resets state', () => {
    vi.useFakeTimers();
    createRoot((dispose) => {
      const { state, applySolverProgress, resetSolverProgress } = createEngineStore();
      applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      vi.advanceTimersByTime(1000);
      expect(state.solverProgress.visible).toBe(true);
      resetSolverProgress();
      expect(state.solverProgress).toEqual({ latest: null, trace: [], visible: false, coarseReached: false });
      dispose();
    });
    vi.useRealTimers();
  });

  it('(step-9a) subscribeToEvents registers onSolverProgress listener', async () => {
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

      expect(mockOnSolverProgress).toHaveBeenCalledWith(expect.any(Function));
      dispose();
    });
  });

  it('(step-9b) onSolverProgress callback drives state.solverProgress', async () => {
    await createRoot(async (dispose) => {
      let progressCb: ((p: { solver_kind: string; iter: number; residual: number }) => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnSolverProgress.mockImplementation(async (cb) => { progressCb = cb as typeof progressCb; return vi.fn(); });

      const store = createEngineStore();
      await store.subscribeToEvents();

      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });
      expect(store.state.solverProgress.latest).toEqual({ solver_kind: 'cg', iter: 1, residual: 0.5 });
      expect(store.state.solverProgress.trace).toHaveLength(1);
      dispose();
    });
  });

  it('(step-9c) cleanup invokes the onSolverProgress unlisten fn', async () => {
    await createRoot(async (dispose) => {
      const unlistenSolverProgress = vi.fn();
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnSolverProgress.mockResolvedValue(unlistenSolverProgress);

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();
      cleanup();
      expect(unlistenSolverProgress).toHaveBeenCalled();
      dispose();
    });
  });

  it('(step-11) cancelSolve calls bridge cancelSolve and resets solverProgress', async () => {
    await createRoot(async (dispose) => {
      vi.useFakeTimers();
      const store = createEngineStore();
      store.applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
      vi.advanceTimersByTime(1000);
      expect(store.state.solverProgress.visible).toBe(true);

      await store.cancelSolve();
      expect(mockCancelSolve).toHaveBeenCalledOnce();
      expect(store.state.solverProgress).toEqual({ latest: null, trace: [], visible: false, coarseReached: false });
      vi.useRealTimers();
      dispose();
    });
  });

  it('(amend-1) cleanup() clears the pending debounce timer so visible never flips after teardown', async () => {
    await createRoot(async (dispose) => {
      vi.useFakeTimers();
      try {
        mockOnMeshUpdate.mockResolvedValue(vi.fn());
        mockOnValueUpdate.mockResolvedValue(vi.fn());
        mockOnConstraintUpdate.mockResolvedValue(vi.fn());
        mockOnEvaluationStatus.mockResolvedValue(vi.fn());
        mockOnMeshRemoved.mockResolvedValue(vi.fn());
        mockOnValueRemoved.mockResolvedValue(vi.fn());
        mockOnConstraintRemoved.mockResolvedValue(vi.fn());

        const store = createEngineStore();
        const cleanup = await store.subscribeToEvents();

        // Arm the debounce timer with one tick
        store.applySolverProgress({ solver_kind: 'cg' as const, iter: 1, residual: 0.5 });
        expect(store.state.solverProgress.visible).toBe(false);

        // Tear down before the timer fires
        cleanup();

        // Timer would fire at 1000ms — cleanup should have cancelled it
        vi.advanceTimersByTime(1000);
        expect(store.state.solverProgress.visible).toBe(false);
      } finally {
        vi.useRealTimers();
      }
      dispose();
    });
  });

  it('(amend-3) trace is capped at 200 entries (sliding window)', () => {
    createRoot((dispose) => {
      const { state, applySolverProgress } = createEngineStore();
      // Push 201 ticks — the 201st should evict the 1st
      for (let i = 1; i <= 201; i++) {
        applySolverProgress({ solver_kind: 'cg' as const, iter: i, residual: 0.5 });
      }
      expect(state.solverProgress.trace).toHaveLength(200);
      // Oldest entry is iter:2 (iter:1 was evicted)
      expect(state.solverProgress.trace[0].iter).toBe(2);
      expect(state.solverProgress.trace[199].iter).toBe(201);
      dispose();
    });
  });
});
