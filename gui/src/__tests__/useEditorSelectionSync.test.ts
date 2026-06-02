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

  it('(i) cross-input race: viewport click during in-flight bridge call is not overwritten', async () => {
    // Scenario:
    // 1. cursor moves → token=1, bridge call in flight
    // 2. viewport click → selectionStore.selectedEntity changes synchronously (no token bump)
    // 3. bridge resolves with an entity that differs from the viewport selection
    // Expected: viewport selection is preserved; bridge result is discarded.
    const { createEditorSelectionSync } = await importHook();

    let resolvePromise!: (v: string | null) => void;
    const promise = new Promise<string | null>(r => { resolvePromise = r; });
    const getEntityAtSourceLocation = vi.fn().mockReturnValue(promise);
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

        // Step 1: cursor moves → bridge call starts after debounce
        editorStore.setCursorPosition(1, 1);
        await vi.advanceTimersByTimeAsync(250);
        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);

        // Step 2: simulate a viewport click updating the selection synchronously
        // (no cursor change → latestRequestToken is NOT bumped)
        selectionStore.selectEntity('NewEntity');
        expect(selectionStore.state.selectedEntity).toBe('NewEntity');

        // Step 3: in-flight bridge call resolves with a different entity
        resolvePromise('OldEntity');
        await Promise.resolve();
        await Promise.resolve();

        // Bridge result must NOT overwrite the viewport-originated selection.
        // The hook's selectEntity callback is never called: the cross-input guard
        // detected that selectionStore.state.selectedEntity changed from null to
        // 'NewEntity' while the bridge call was in flight and discarded the result.
        expect(selectEntity).not.toHaveBeenCalled();
        // The state still reflects the viewport selection.
        expect(selectionStore.state.selectedEntity).toBe('NewEntity');

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

  it('(j) cross-input race during debounce: viewport click before debounce fires is not overwritten', async () => {
    // Scenario (the during-debounce race — distinct from the in-flight-bridge race tested in (i)):
    // 1. cursor moves at t=0 → token=1, 200ms debounce timer starts, bridge NOT yet called
    // 2. at t=100 (still within debounce window): viewport click sets selectedEntity = 'ViewportEntity'
    // 3. debounce fires at t=200 → short-circuits before bridge call: selection already changed
    // Expected: bridge is never called; viewport selection is preserved.
    //
    // With the buggy code, selectionBeforeAwait was captured INSIDE the setTimeout callback
    // (at t=200) by which point selectedEntity was already 'ViewportEntity', so the guard
    // saw 'ViewportEntity' !== 'ViewportEntity' → false → passed → selectEntity was wrongly called.
    const { createEditorSelectionSync } = await importHook();

    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('EditorEntity');
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

        // Step 1: cursor moves at t=0 → debounce starts; bridge NOT yet called
        editorStore.setCursorPosition(5, 10);
        await vi.advanceTimersByTimeAsync(100); // t=100 — mid-debounce
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();

        // Step 2: viewport click during the debounce window (no cursor change → no token bump)
        selectionStore.selectEntity('ViewportEntity');
        expect(selectionStore.state.selectedEntity).toBe('ViewportEntity');

        // Step 3: advance past debounce (t=250) → setTimeout fires, but short-circuits before
        // awaiting the bridge because selection already changed during the debounce window.
        await vi.advanceTimersByTimeAsync(150);

        // Bridge NOT called: the pre-await early-exit fired before getEntityAtSourceLocation
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();

        // The hook's selectEntity prop must NOT have been called
        expect(selectEntity).not.toHaveBeenCalled();
        expect(flyToEntity).not.toHaveBeenCalled();

        // Store still reflects the viewport-originated selection
        expect(selectionStore.state.selectedEntity).toBe('ViewportEntity');

        dispose();
        done();
      });
    });
  });
});

// ── createEditorHoverSync tests ──────────────────────────────────────────────

function makeHoverEditorStore(initial: { line: number; column: number } | null = null) {
  const [state, setState] = createStore({ cursorPosition: initial as { line: number; column: number } | null });
  return {
    state,
    setCursorPosition: (pos: { line: number; column: number } | null) => setState('cursorPosition', pos),
  };
}

describe('createEditorHoverSync', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('(a) cursor set → after 200ms, getEntityAtSourceLocation called; hoverEntity called with result', async () => {
    const { createEditorHoverSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.width');
    const hoverEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeHoverEditorStore(null);

        createEditorHoverSync({
          editorStore,
          getEntityAtSourceLocation,
          hoverEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 2, column: 11 });

        // Before timer fires: no call yet
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();

        await vi.advanceTimersByTimeAsync(250);

        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);
        expect(getEntityAtSourceLocation).toHaveBeenCalledWith(2, 11);
        expect(hoverEntity).toHaveBeenCalledWith('Bracket.width');

        dispose();
        done();
      });
    });
  });

  it('(b) two rapid cursor changes → exactly one bridge call with the last position', async () => {
    const { createEditorHoverSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.width');
    const hoverEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeHoverEditorStore(null);

        createEditorHoverSync({
          editorStore,
          getEntityAtSourceLocation,
          hoverEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 1, column: 1 });
        await vi.advanceTimersByTimeAsync(100); // within debounce window
        editorStore.setCursorPosition({ line: 5, column: 8 });
        await vi.advanceTimersByTimeAsync(250); // fire second debounce

        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);
        expect(getEntityAtSourceLocation).toHaveBeenCalledWith(5, 8);

        dispose();
        done();
      });
    });
  });

  it('(c) cursor set to null → hoverEntity called with null without waiting for debounce', async () => {
    const { createEditorHoverSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue('Bracket.width');
    const hoverEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeHoverEditorStore({ line: 2, column: 5 });

        createEditorHoverSync({
          editorStore,
          getEntityAtSourceLocation,
          hoverEntity,
          debounceMs: 200,
        });

        // Set cursor to null (hover-off)
        editorStore.setCursorPosition(null);

        // Flush SolidJS effects (they run as microtasks); no timer advance needed
        await vi.advanceTimersByTimeAsync(0);

        // hoverEntity should be called with null — no full debounce wait required
        expect(hoverEntity).toHaveBeenCalledWith(null);
        // No bridge call should be made for a null cursor
        expect(getEntityAtSourceLocation).not.toHaveBeenCalled();

        dispose();
        done();
      });
    });
  });

  it('(d) bridge resolves null → hoverEntity called with null (cursor on whitespace)', async () => {
    const { createEditorHoverSync } = await importHook();
    const getEntityAtSourceLocation = vi.fn().mockResolvedValue(null);
    const hoverEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeHoverEditorStore(null);

        createEditorHoverSync({
          editorStore,
          getEntityAtSourceLocation,
          hoverEntity,
          debounceMs: 200,
        });

        editorStore.setCursorPosition({ line: 1, column: 1 });
        await vi.advanceTimersByTimeAsync(250);

        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);
        // null resolution → hoverEntity must be called with null to clear hover
        expect(hoverEntity).toHaveBeenCalledWith(null);

        dispose();
        done();
      });
    });
  });

  it('(e) stale-result race guard: slow request #1 result discarded when fast request #2 resolves first', async () => {
    // Mirror of sibling hook test (h):
    // Call #1 → slow promise; call #2 → fast promise.
    // Fast (#2) resolves with 'Bracket.width' → hoverEntity called once.
    // Slow (#1, stale) resolves with 'Bracket.thickness' → hoverEntity NOT called again.
    const { createEditorHoverSync } = await importHook();

    let resolveSlowPromise!: (v: string | null) => void;
    let resolveFastPromise!: (v: string | null) => void;
    const slowPromise = new Promise<string | null>(r => { resolveSlowPromise = r; });
    const fastPromise = new Promise<string | null>(r => { resolveFastPromise = r; });

    let callCount = 0;
    const getEntityAtSourceLocation = vi.fn().mockImplementation(() => {
      callCount++;
      return callCount === 1 ? slowPromise : fastPromise;
    });
    const hoverEntity = vi.fn();

    await new Promise<void>((done) => {
      createRoot(async (dispose) => {
        const editorStore = makeHoverEditorStore(null);

        createEditorHoverSync({
          editorStore,
          getEntityAtSourceLocation,
          hoverEntity,
          debounceMs: 200,
        });

        // Change #1 → fires debounce #1 → awaits slowPromise
        editorStore.setCursorPosition({ line: 1, column: 1 });
        await vi.advanceTimersByTimeAsync(250);
        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(1);

        // Change #2 → fires debounce #2 → awaits fastPromise
        editorStore.setCursorPosition({ line: 2, column: 2 });
        await vi.advanceTimersByTimeAsync(250);
        expect(getEntityAtSourceLocation).toHaveBeenCalledTimes(2);

        // Resolve fast promise (call #2) first with 'Bracket.width'
        resolveFastPromise('Bracket.width');
        await Promise.resolve();
        await Promise.resolve();

        // Fresh (call #2) result should have been applied
        expect(hoverEntity).toHaveBeenCalledWith('Bracket.width');
        const callsBefore = hoverEntity.mock.calls.length;

        // Now resolve slow (stale) promise with 'Bracket.thickness'
        resolveSlowPromise('Bracket.thickness');
        await Promise.resolve();
        await Promise.resolve();

        // Stale result MUST be discarded — hoverEntity must NOT be called again
        expect(hoverEntity.mock.calls.length).toBe(callsBefore);

        dispose();
        done();
      });
    });
  });
});
