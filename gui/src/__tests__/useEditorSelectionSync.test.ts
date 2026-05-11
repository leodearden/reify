import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { createStore } from 'solid-js/store';

// ── Minimal mock stores ──────────────────────────────────────────────────────

function makeEditorStore(initial: { line: number; column: number } | null = null) {
  const [state, setState] = createStore({ cursorPosition: initial as { line: number; column: number } | null });
  return {
    state,
    setCursorPosition: (line: number, column: number) => setState('cursorPosition', { line, column }),
  };
}

function makeSelectionStore(initialEntity: string | null = null) {
  const [state, setState] = createStore({ selectedEntity: initialEntity as string | null });
  const selectEntity = vi.fn((path: string | null) => setState('selectedEntity', path));
  return { state, selectEntity };
}

// ── Lazy import ──────────────────────────────────────────────────────────────

async function importHook() {
  return import('../hooks/useEditorSelectionSync');
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('createEditorSelectionSync', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('(a) cursorPosition null → no bridge call after timer', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn();
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore(null);

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        await vi.advanceTimersByTimeAsync(500);
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();
        dispose();
        done();
      });
    });
  });

  it('(b) cursor change → after 200ms, bridge called with (line, column)', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue(null);
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore(null);

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(7, 12);

        // Before timer fires: no call yet
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();

        // Advance past debounce
        await vi.advanceTimersByTimeAsync(250);

        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);
        expect(getEntityAtSourceLocation).toHaveBeenCalledWith(7, 12);

        dispose();
        done();
      });
    });
  });

  it('(c) two rapid cursor changes → exactly one bridge call with the last position', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue(null);
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore(null);

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(1, 1);
        await vi.advanceTimersByTimeAsync(100); // within debounce window
        editorStore.setCursorPosition(5, 8);
        await vi.advanceTimersByTimeAsync(250); // fire second debounce

        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);
        expect(getEntityAtSourceLocation).toHaveBeenCalledWith(5, 8);

        dispose();
        done();
      });
    });
  });

  it('(d) bridge returns "Bracket.width" → selectEntity and flyToEntity called', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.width');
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore(null);

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(2, 11);
        await vi.advanceTimersByTimeAsync(250);

        expect(selectEntity).toHaveBeenCalledWith('Bracket.width');
        expect(flyToEntity).toHaveBeenCalledWith('Bracket.width');

        dispose();
        done();
      });
    });
  });

  it('(e) bridge returns null → neither selectEntity nor flyToEntity called', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue(null);
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore('Bracket'); // already has a selection

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(1, 1);
        await vi.advanceTimersByTimeAsync(250);

        // null result → existing selection preserved, no mutation
        expect(selectEntity).not.toHaveBeenCalled();
        expect(flyToEntity).not.toHaveBeenCalled();

        dispose();
        done();
      });
    });
  });

  it('(f) bridge returns the same entity as current selectedEntity → neither selectEntity nor flyToEntity called (equality-check guard)', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.width');
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore('Bracket.width'); // same as what bridge returns

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(2, 11);
        await vi.advanceTimersByTimeAsync(250);

        // equal entity → skip mutation to prevent feedback loop
        expect(selectEntity).not.toHaveBeenCalled();
        expect(flyToEntity).not.toHaveBeenCalled();

        dispose();
        done();
      });
    });
  });

  it('(g) bridge returns a different entity from current → both selectEntity and flyToEntity called with new entity', async () => {
    const { createEditorSelectionSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.thickness');
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore('Bracket.width'); // different from what bridge returns

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition(4, 11);
        await vi.advanceTimersByTimeAsync(250);

        expect(selectEntity).toHaveBeenCalledWith('Bracket.thickness');
        expect(flyToEntity).toHaveBeenCalledWith('Bracket.thickness');

        dispose();
        done();
      });
    });
  });

  it('(h) race condition guard: slow request #1 then fast request #2; only #2 result applied', async () => {
    const { createEditorSelectionSync } = await importHook();

    let resolveSlowPromise!: (v: string | null) => void;
    let resolveFastPromise!: (v: string | null) => void;
    const slowPromise = new Promise<string | null>(r => { resolveSlowPromise = r; });
    const fastPromise = new Promise<string | null>(r => { resolveFastPromise = r; });

    let callCount = 0;
    const getEntityAtSourceLocation = vi.fn().mockImplementation(() => {
      callCount++;
      return callCount === 1 ? slowPromise : fastPromise;
    });
    const selectEntity = vi.fn();
    const flyToEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeEditorStore(null);
        const selectionStore = makeSelectionStore(null);

        createEditorSelectionSync({
          editorStore,
          selectionStore,
          getEntityAtSourceLocation,
          selectEntity,
          flyToEntity,
          debounceMs: 200,
        });

        // Change #1 → fires debounce #1 → awaits slowPromise
        editorStore.setCursorPosition(1, 1);
        await vi.advanceTimersByTimeAsync(250);
        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);

        // Change #2 → fires debounce #2 → awaits fastPromise
        editorStore.setCursorPosition(2, 2);
        await vi.advanceTimersByTimeAsync(250);
        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(2);

        // Resolve fast promise (call #2) first with 'Bracket.width'
        resolveFastPromise('Bracket.width');
        await Promise.resolve();
        await Promise.resolve();

        // Fresh (call #2) result should have been applied
        expect(selectEntity).toHaveBeenCalledWith('Bracket.width');
        expect(flyToEntity).toHaveBeenCalledWith('Bracket.width');

        const selectEntityCallsBefore = selectEntity.mock.calls.length;
        const flyToEntityCallsBefore = flyToEntity.mock.calls.length;

        // Now resolve slow (stale) promise with 'Bracket.thickness'
        resolveSlowPromise('Bracket.thickness');
        await Promise.resolve();
        await Promise.resolve();

        // Stale result should have been DISCARDED
        expect(selectEntity.mock.calls.length).toBe(selectEntityCallsBefore);
        expect(flyToEntity.mock.calls.length).toBe(flyToEntityCallsBefore);

        dispose();
        done();
      });
    });
  });
});
