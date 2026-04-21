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
      createRoot((dispose) => {
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

        vi.advanceTimersByTime(500);
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
});
