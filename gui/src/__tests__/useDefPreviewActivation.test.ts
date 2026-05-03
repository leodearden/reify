import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { createStore } from 'solid-js/store';
import type { DefInfo } from '../types';

// ── Minimal mock stores ──────────────────────────────────────────────────────

function makeEditorStore(initial: { line: number; column: number } | null = null) {
  const [state, setState] = createStore({ cursorPosition: initial as { line: number; column: number } | null });
  return { state, setCursorPosition: (pos: typeof state.cursorPosition) => setState('cursorPosition', pos) };
}

function makeViewportStore() {
  const setDefPath = vi.fn().mockReturnValue(true);
  const setForceExpanded = vi.fn().mockReturnValue(true);
  const state = { viewports: { 'def-preview': { defPath: null } } };
  return { state, setDefPath, setForceExpanded };
}

function makeDefPreviewStore() {
  const state = { defName: null as string | null, meshes: {}, isLoading: false, error: null };
  const loadPreview = vi.fn().mockResolvedValue(undefined);
  const clearPreview = vi.fn();
  return { state, loadPreview, clearPreview };
}

// ── Lazy import ──────────────────────────────────────────────────────────────

async function importHook() {
  return import('../hooks/useDefPreviewActivation');
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('createDefPreviewActivation', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('(a) when cursorPosition is null, getContainingDefinition is never called after timer', async () => {
    const { createDefPreviewActivation } = await importHook();
    const getContainingDefinition = vi.fn();
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        await vi.advanceTimersByTimeAsync(500);
        expect(getContainingDefinition).not.toHaveBeenCalled();
        dispose();
        done();
      });
    });
  });

  it('(b) on cursor change, after 200ms, getContainingDefinition is called with (line, column)', async () => {
    const { createDefPreviewActivation } = await importHook();
    const getContainingDefinition = vi.fn().mockResolvedValue(null);
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 7, column: 12 });

        // Before timer fires: no call yet
        expect(getContainingDefinition).not.toHaveBeenCalled();

        // Advance past debounce
        await vi.advanceTimersByTimeAsync(250);

        expect(getContainingDefinition).toHaveBeenCalledTimes(1);
        expect(getContainingDefinition).toHaveBeenCalledWith(7, 12);

        dispose();
        done();
      });
    });
  });

  it('(c) two rapid cursor changes result in exactly one getContainingDefinition call (last position)', async () => {
    const { createDefPreviewActivation } = await importHook();
    const getContainingDefinition = vi.fn().mockResolvedValue(null);
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 1, column: 1 });
        await vi.advanceTimersByTimeAsync(100); // within debounce window
        editorStore.setCursorPosition({ line: 5, column: 8 });
        await vi.advanceTimersByTimeAsync(250); // fire second debounce

        expect(getContainingDefinition).toHaveBeenCalledTimes(1);
        expect(getContainingDefinition).toHaveBeenCalledWith(5, 8);

        dispose();
        done();
      });
    });
  });

  it('(d) when getContainingDefinition resolves to DefInfo, setDefPath and loadPreview are called', async () => {
    const { createDefPreviewActivation } = await importHook();
    const defInfo: DefInfo = { name: 'BoltFlange', kind: 'structure', span: { start: 0, end: 100 } };
    const getContainingDefinition = vi.fn().mockResolvedValue(defInfo);
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 3, column: 5 });
        await vi.advanceTimersByTimeAsync(250);

        expect(viewportStore.setDefPath).toHaveBeenCalledWith('def-preview', 'BoltFlange');
        expect(defPreviewStore.loadPreview).toHaveBeenCalledWith('BoltFlange', getDefPreview);

        dispose();
        done();
      });
    });
  });

  it('(e) when getContainingDefinition resolves to null, setDefPath(null) and clearPreview are called', async () => {
    const { createDefPreviewActivation } = await importHook();
    const getContainingDefinition = vi.fn().mockResolvedValue(null);
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 2, column: 3 });
        await vi.advanceTimersByTimeAsync(250);

        expect(viewportStore.setDefPath).toHaveBeenCalledWith('def-preview', null);
        expect(defPreviewStore.clearPreview).toHaveBeenCalledTimes(1);

        dispose();
        done();
      });
    });
  });

  it('(f) defPreviewActive() returns true when DefInfo is set, false otherwise', async () => {
    const { createDefPreviewActivation } = await importHook();
    const defInfo: DefInfo = { name: 'BoltFlange', kind: 'structure', span: { start: 0, end: 100 } };
    const getContainingDefinition = vi.fn().mockResolvedValue(defInfo);
    const getDefPreview = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const viewportStore = makeViewportStore();
        const defPreviewStore = makeDefPreviewStore();

        const { defPreviewActive } = createDefPreviewActivation({
          editorStore,
          viewportStore,
          defPreviewStore,
          getContainingDefinition,
          getDefPreview,
          debounceMs: 200,
        });

        // Initially no DefInfo
        expect(defPreviewActive()).toBe(false);

        editorStore.setCursorPosition({ line: 3, column: 5 });
        await vi.advanceTimersByTimeAsync(250);

        expect(defPreviewActive()).toBe(true);

        dispose();
        done();
      });
    });
  });

  // ── Race condition tests ─────────────────────────────────────────────────────

  describe('race condition', () => {
    it('stale getContainingDefinition result (slow call #1) is discarded when newer (fast call #2) resolves first', async () => {
      const { createDefPreviewActivation } = await importHook();

      // Create two deferred promises so we control resolution order
      let resolveSlowPromise!: (v: DefInfo | null) => void;
      let resolveFastPromise!: (v: DefInfo | null) => void;
      const slowPromise = new Promise<DefInfo | null>(r => { resolveSlowPromise = r; });
      const fastPromise = new Promise<DefInfo | null>(r => { resolveFastPromise = r; });

      let callCount = 0;
      const getContainingDefinition = vi.fn().mockImplementation(() => {
        callCount++;
        return callCount === 1 ? slowPromise : fastPromise;
      });
      const getDefPreview = vi.fn();

      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const editorStore = makeEditorStore(null);
          const viewportStore = makeViewportStore();
          const defPreviewStore = makeDefPreviewStore();

          const { defInfo } = createDefPreviewActivation({
            editorStore,
            viewportStore,
            defPreviewStore,
            getContainingDefinition,
            getDefPreview,
            debounceMs: 200,
          });

          // Change #1 → fires debounce #1 → awaits slowPromise
          editorStore.setCursorPosition({ line: 1, column: 1 });
          await vi.advanceTimersByTimeAsync(250);
          expect(getContainingDefinition).toHaveBeenCalledTimes(1);

          // Change #2 → fires debounce #2 → awaits fastPromise
          editorStore.setCursorPosition({ line: 2, column: 2 });
          await vi.advanceTimersByTimeAsync(250);
          expect(getContainingDefinition).toHaveBeenCalledTimes(2);

          const defInfoB: DefInfo = { name: 'B', kind: 'structure', span: { start: 0, end: 10 } };
          const defInfoA: DefInfo = { name: 'A', kind: 'structure', span: { start: 0, end: 10 } };

          // Resolve fast promise (call #2) first with DefInfo B
          resolveFastPromise(defInfoB);
          await Promise.resolve();
          await Promise.resolve();

          // Fresh (call #2) result should have been applied
          expect(viewportStore.setDefPath).toHaveBeenCalledWith('def-preview', 'B');
          expect(defPreviewStore.loadPreview).toHaveBeenCalledWith('B', getDefPreview);
          expect(defInfo()?.name).toBe('B');

          const setDefPathCallsBefore = viewportStore.setDefPath.mock.calls.length;
          const loadPreviewCallsBefore = (defPreviewStore.loadPreview as ReturnType<typeof vi.fn>).mock.calls.length;

          // Now resolve slow (stale) promise with DefInfo A
          resolveSlowPromise(defInfoA);
          await Promise.resolve();
          await Promise.resolve();

          // Stale result should have been DISCARDED
          expect(viewportStore.setDefPath.mock.calls.length).toBe(setDefPathCallsBefore);
          expect((defPreviewStore.loadPreview as ReturnType<typeof vi.fn>).mock.calls.length).toBe(loadPreviewCallsBefore);
          // defInfo() should still reflect the fresh result
          expect(defInfo()?.name).toBe('B');

          dispose();
          done();
        });
      });
    });

    it('cursor moving mid-flight (after timer fires, before next timer fires) invalidates the in-flight getContainingDefinition', async () => {
      const { createDefPreviewActivation } = await importHook();

      // Two deferred promises: first is slow (still in-flight when cursor moves),
      // second fires only after the second debounce.
      let resolveFirstPromise!: (v: DefInfo | null) => void;
      let resolveSecondPromise!: (v: DefInfo | null) => void;
      const firstPromise = new Promise<DefInfo | null>(r => { resolveFirstPromise = r; });
      const secondPromise = new Promise<DefInfo | null>(r => { resolveSecondPromise = r; });

      let callCount = 0;
      const getContainingDefinition = vi.fn().mockImplementation(() => {
        callCount++;
        return callCount === 1 ? firstPromise : secondPromise;
      });
      const getDefPreview = vi.fn();

      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const editorStore = makeEditorStore(null);
          const viewportStore = makeViewportStore();
          const defPreviewStore = makeDefPreviewStore();

          const { defInfo } = createDefPreviewActivation({
            editorStore,
            viewportStore,
            defPreviewStore,
            getContainingDefinition,
            getDefPreview,
            debounceMs: 200,
          });

          // Step 1: set cursor to pos A
          editorStore.setCursorPosition({ line: 1, column: 1 });

          // Step 2: advance past debounce — first timer fires; getContainingDefinition
          // is called and awaiting firstPromise. T1's callback has cleared timerId.
          await vi.advanceTimersByTimeAsync(250);
          expect(getContainingDefinition).toHaveBeenCalledTimes(1);

          // Step 3: cursor moves to pos B — second debounce scheduled.
          // T1 is still awaiting firstPromise; T2 has NOT fired yet.
          editorStore.setCursorPosition({ line: 2, column: 2 });

          // Step 4: resolve the *first* (now stale) deferred with DefInfo for 'A'
          const defInfoA: DefInfo = { name: 'A', kind: 'structure', span: { start: 0, end: 10 } };
          resolveFirstPromise(defInfoA);
          await Promise.resolve();
          await Promise.resolve();

          // The stale 'A' result must NOT have been applied:
          expect(viewportStore.setDefPath).not.toHaveBeenCalledWith('def-preview', 'A');
          expect(defPreviewStore.loadPreview).not.toHaveBeenCalledWith('A', getDefPreview);
          expect(defInfo()).toBeNull();

          // Step 5: fire the second debounce and resolve with null — no stale 'A' effects
          await vi.advanceTimersByTimeAsync(250);
          resolveSecondPromise(null);
          await Promise.resolve();
          await Promise.resolve();

          // After the second request resolves null, setDefPath(null) and clearPreview
          // should be called — but no 'A' effects should have fired at any point
          expect(viewportStore.setDefPath).not.toHaveBeenCalledWith('def-preview', 'A');
          expect(defPreviewStore.loadPreview).not.toHaveBeenCalledWith('A', getDefPreview);

          dispose();
          done();
        });
      });
    });

    it('stale null resolution does not clear preview established by a newer call', async () => {
      const { createDefPreviewActivation } = await importHook();

      let resolveSlowPromise!: (v: DefInfo | null) => void;
      let resolveFastPromise!: (v: DefInfo | null) => void;
      const slowPromise = new Promise<DefInfo | null>(r => { resolveSlowPromise = r; });
      const fastPromise = new Promise<DefInfo | null>(r => { resolveFastPromise = r; });

      let callCount = 0;
      const getContainingDefinition = vi.fn().mockImplementation(() => {
        callCount++;
        return callCount === 1 ? slowPromise : fastPromise;
      });
      const getDefPreview = vi.fn();

      await new Promise<void>((done) => {
        createRoot(async (dispose) => {
          const editorStore = makeEditorStore(null);
          const viewportStore = makeViewportStore();
          const defPreviewStore = makeDefPreviewStore();

          const { defInfo } = createDefPreviewActivation({
            editorStore,
            viewportStore,
            defPreviewStore,
            getContainingDefinition,
            getDefPreview,
            debounceMs: 200,
          });

          // Trigger two cursor changes
          editorStore.setCursorPosition({ line: 1, column: 1 });
          await vi.advanceTimersByTimeAsync(250);

          editorStore.setCursorPosition({ line: 2, column: 2 });
          await vi.advanceTimersByTimeAsync(250);

          const defInfoB: DefInfo = { name: 'B', kind: 'structure', span: { start: 0, end: 10 } };

          // Fast promise (call #2) resolves with DefInfo B
          resolveFastPromise(defInfoB);
          await Promise.resolve();
          await Promise.resolve();

          expect(defInfo()?.name).toBe('B');

          // Slow promise (call #1) resolves with null — stale null
          resolveSlowPromise(null);
          await Promise.resolve();
          await Promise.resolve();

          // clearPreview should NOT have been called (stale null discarded)
          expect(defPreviewStore.clearPreview).not.toHaveBeenCalled();
          // defInfo() should still reflect the fresh B result
          expect(defInfo()?.name).toBe('B');

          dispose();
          done();
        });
      });
    });
  });
});
