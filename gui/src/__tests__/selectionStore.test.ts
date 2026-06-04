import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { batch, createRoot } from 'solid-js';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { createSelectionStore } from '../stores/selectionStore';

const mockInvoke = vi.mocked(invoke);

describe('selectionStore', () => {
  it('has null selectedEntity and hoveredEntity initially', () => {
    createRoot((dispose) => {
      const { state } = createSelectionStore();
      expect(state.selectedEntity).toBeNull();
      expect(state.hoveredEntity).toBeNull();
      dispose();
    });
  });

  it('has empty selectedEntities array initially', () => {
    createRoot((dispose) => {
      const { state } = createSelectionStore();
      expect(state.selectedEntities).toEqual([]);
      dispose();
    });
  });

  it('has null anchorEntity initially', () => {
    createRoot((dispose) => {
      const { state } = createSelectionStore();
      expect(state.anchorEntity).toBeNull();
      dispose();
    });
  });

  describe('selectSingle', () => {
    it('sets selectedEntities to [path]', () => {
      createRoot((dispose) => {
        const { state, selectSingle } = createSelectionStore();
        selectSingle('Bracket');
        expect(state.selectedEntities).toEqual(['Bracket']);
        dispose();
      });
    });

    it('sets anchorEntity to path', () => {
      createRoot((dispose) => {
        const { state, selectSingle } = createSelectionStore();
        selectSingle('Bracket');
        expect(state.anchorEntity).toBe('Bracket');
        dispose();
      });
    });

    it('sets selectedEntity (primary) to path', () => {
      createRoot((dispose) => {
        const { state, selectSingle } = createSelectionStore();
        selectSingle('Bracket');
        expect(state.selectedEntity).toBe('Bracket');
        dispose();
      });
    });

    it('calling selectSingle again replaces (not appends) the selection', () => {
      createRoot((dispose) => {
        const { state, selectSingle } = createSelectionStore();
        selectSingle('Bracket');
        selectSingle('Mount');
        expect(state.selectedEntities).toEqual(['Mount']);
        expect(state.selectedEntity).toBe('Mount');
        expect(state.anchorEntity).toBe('Mount');
        dispose();
      });
    });

    it('selectSingle(null) empties selection and clears anchor', () => {
      createRoot((dispose) => {
        const { state, selectSingle } = createSelectionStore();
        selectSingle('Bracket');
        selectSingle(null);
        expect(state.selectedEntities).toEqual([]);
        expect(state.selectedEntity).toBeNull();
        expect(state.anchorEntity).toBeNull();
        dispose();
      });
    });

    it('selectSingle resets highlightedParams when previously seeded', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, selectSingle } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        selectSingle('Bracket');
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });

    it('selectSingle(null) resets highlightedParams when previously seeded', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, selectSingle } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        selectSingle(null);
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });
  });

  describe('toggleSelect', () => {
    it('adds path when absent, appends to selectedEntities, primary=path', () => {
      createRoot((dispose) => {
        const { state, toggleSelect } = createSelectionStore();
        toggleSelect('Bracket');
        expect(state.selectedEntities).toEqual(['Bracket']);
        expect(state.selectedEntity).toBe('Bracket');
        dispose();
      });
    });

    it('removes path when present, primary updates to last remaining', () => {
      createRoot((dispose) => {
        const { state, toggleSelect } = createSelectionStore();
        toggleSelect('Bracket');
        toggleSelect('Mount');
        // now both are in: ['Bracket', 'Mount']
        toggleSelect('Mount');
        // Mount removed, only Bracket remains
        expect(state.selectedEntities).toEqual(['Bracket']);
        expect(state.selectedEntity).toBe('Bracket');
        dispose();
      });
    });

    it('removing last element sets primary to null', () => {
      createRoot((dispose) => {
        const { state, toggleSelect } = createSelectionStore();
        toggleSelect('Bracket');
        toggleSelect('Bracket');
        expect(state.selectedEntities).toEqual([]);
        expect(state.selectedEntity).toBeNull();
        dispose();
      });
    });

    it('preserves insertion order of other entries', () => {
      createRoot((dispose) => {
        const { state, toggleSelect } = createSelectionStore();
        toggleSelect('A');
        toggleSelect('B');
        toggleSelect('C');
        toggleSelect('B'); // remove middle
        expect(state.selectedEntities).toEqual(['A', 'C']);
        dispose();
      });
    });

    it('does not change anchorEntity', () => {
      createRoot((dispose) => {
        const { state, selectSingle, toggleSelect } = createSelectionStore();
        selectSingle('Bracket');
        expect(state.anchorEntity).toBe('Bracket');
        toggleSelect('Mount');
        // anchor should still be Bracket (only selectSingle sets anchor)
        expect(state.anchorEntity).toBe('Bracket');
        dispose();
      });
    });

    it('toggleSelect resets highlightedParams when previously seeded', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, toggleSelect } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        toggleSelect('Bracket');
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });
  });

  describe('rangeSelect', () => {
    it('replaces selectedEntities with provided paths (dedup preserving first occurrence)', () => {
      createRoot((dispose) => {
        const { state, rangeSelect } = createSelectionStore();
        rangeSelect(['A', 'B', 'A', 'C']);
        expect(state.selectedEntities).toEqual(['A', 'B', 'C']);
        dispose();
      });
    });

    it('sets selectedEntity to last path (primary=last)', () => {
      createRoot((dispose) => {
        const { state, rangeSelect } = createSelectionStore();
        rangeSelect(['A', 'B', 'C']);
        expect(state.selectedEntity).toBe('C');
        dispose();
      });
    });

    it('does not change anchorEntity', () => {
      createRoot((dispose) => {
        const { state, selectSingle, rangeSelect } = createSelectionStore();
        selectSingle('X');
        expect(state.anchorEntity).toBe('X');
        rangeSelect(['A', 'B']);
        expect(state.anchorEntity).toBe('X');
        dispose();
      });
    });

    it('empty paths list empties selection and sets primary to null', () => {
      createRoot((dispose) => {
        const { state, rangeSelect } = createSelectionStore();
        rangeSelect([]);
        expect(state.selectedEntities).toEqual([]);
        expect(state.selectedEntity).toBeNull();
        dispose();
      });
    });

    it('rangeSelect resets highlightedParams when previously seeded', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, rangeSelect } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        rangeSelect(['A', 'B']);
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });
  });

  describe('clearSelection', () => {
    it('empties selectedEntities', () => {
      createRoot((dispose) => {
        const { state, selectSingle, clearSelection } = createSelectionStore();
        selectSingle('A');
        clearSelection();
        expect(state.selectedEntities).toEqual([]);
        dispose();
      });
    });

    it('sets selectedEntity to null', () => {
      createRoot((dispose) => {
        const { state, selectSingle, clearSelection } = createSelectionStore();
        selectSingle('A');
        clearSelection();
        expect(state.selectedEntity).toBeNull();
        dispose();
      });
    });

    it('sets anchorEntity to null', () => {
      createRoot((dispose) => {
        const { state, selectSingle, clearSelection } = createSelectionStore();
        selectSingle('A');
        clearSelection();
        expect(state.anchorEntity).toBeNull();
        dispose();
      });
    });

    it('clears highlightedParams', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, clearSelection } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        clearSelection();
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });
  });

  it('selectEntity sets selectedEntity', () => {
    createRoot((dispose) => {
      const { state, selectEntity } = createSelectionStore();
      selectEntity('Bracket');
      expect(state.selectedEntity).toBe('Bracket');
      dispose();
    });
  });

  describe('selectAll', () => {
    it('replaces selectedEntities with provided list (dedup)', () => {
      createRoot((dispose) => {
        const { state, selectAll } = createSelectionStore();
        selectAll(['A', 'B', 'A', 'C']);
        expect(state.selectedEntities).toEqual(['A', 'B', 'C']);
        dispose();
      });
    });

    it('sets selectedEntity to last item', () => {
      createRoot((dispose) => {
        const { state, selectAll } = createSelectionStore();
        selectAll(['A', 'B', 'C']);
        expect(state.selectedEntity).toBe('C');
        dispose();
      });
    });

    it('does not change anchorEntity', () => {
      createRoot((dispose) => {
        const { state, selectSingle, selectAll } = createSelectionStore();
        selectSingle('X');
        expect(state.anchorEntity).toBe('X');
        selectAll(['A', 'B', 'C']);
        expect(state.anchorEntity).toBe('X');
        dispose();
      });
    });

    it('selectAll resets highlightedParams when previously seeded', () => {
      createRoot((dispose) => {
        const { state, setHighlightedParams, selectAll } = createSelectionStore();
        setHighlightedParams(['c1', 'c2']);
        selectAll(['A', 'B']);
        expect(state.highlightedParams).toEqual([]);
        dispose();
      });
    });
  });

  describe('selectEntity backward-compat alias', () => {
    it('selectEntity(path) sets selectedEntities=[path] (same as selectSingle)', () => {
      createRoot((dispose) => {
        const { state, selectEntity } = createSelectionStore();
        selectEntity('Bracket');
        expect(state.selectedEntities).toEqual(['Bracket']);
        dispose();
      });
    });

    it('selectEntity(path) sets anchorEntity=path (same as selectSingle)', () => {
      createRoot((dispose) => {
        const { state, selectEntity } = createSelectionStore();
        selectEntity('Bracket');
        expect(state.anchorEntity).toBe('Bracket');
        dispose();
      });
    });

    it('selectEntity(null) empties selectedEntities (same as clearSelection)', () => {
      createRoot((dispose) => {
        const { state, selectEntity } = createSelectionStore();
        selectEntity('Bracket');
        selectEntity(null);
        expect(state.selectedEntities).toEqual([]);
        dispose();
      });
    });

    it('selectEntity(null) clears anchorEntity (same as clearSelection)', () => {
      createRoot((dispose) => {
        const { state, selectEntity } = createSelectionStore();
        selectEntity('Bracket');
        selectEntity(null);
        expect(state.anchorEntity).toBeNull();
        dispose();
      });
    });
  });

  it('selectEntity(null) clears selection', () => {
    createRoot((dispose) => {
      const { state, selectEntity } = createSelectionStore();
      selectEntity('Bracket');
      selectEntity(null);
      expect(state.selectedEntity).toBeNull();
      dispose();
    });
  });

  it('hoverEntity sets hoveredEntity', () => {
    createRoot((dispose) => {
      const { state, hoverEntity } = createSelectionStore();
      hoverEntity('Bracket.width');
      expect(state.hoveredEntity).toBe('Bracket.width');
      dispose();
    });
  });

  it('hoverEntity(null) clears hover', () => {
    createRoot((dispose) => {
      const { state, hoverEntity } = createSelectionStore();
      hoverEntity('Bracket.width');
      hoverEntity(null);
      expect(state.hoveredEntity).toBeNull();
      dispose();
    });
  });

  it('selecting and hovering are independent', () => {
    createRoot((dispose) => {
      const { state, selectEntity, hoverEntity } = createSelectionStore();
      selectEntity('Bracket');
      hoverEntity('Bracket.width');
      expect(state.selectedEntity).toBe('Bracket');
      expect(state.hoveredEntity).toBe('Bracket.width');

      selectEntity(null);
      expect(state.selectedEntity).toBeNull();
      expect(state.hoveredEntity).toBe('Bracket.width');

      hoverEntity(null);
      expect(state.hoveredEntity).toBeNull();
      dispose();
    });
  });

  it('highlightedParams defaults to empty array', () => {
    createRoot((dispose) => {
      const { state } = createSelectionStore();
      expect(state.highlightedParams).toEqual([]);
      dispose();
    });
  });

  it('setHighlightedParams sets highlightedParams', () => {
    createRoot((dispose) => {
      const { state, setHighlightedParams } = createSelectionStore();
      setHighlightedParams(['c1', 'c2']);
      expect(state.highlightedParams).toEqual(['c1', 'c2']);
      dispose();
    });
  });

  it('setHighlightedParams([]) clears highlightedParams', () => {
    createRoot((dispose) => {
      const { state, setHighlightedParams } = createSelectionStore();
      setHighlightedParams(['c1', 'c2']);
      setHighlightedParams([]);
      expect(state.highlightedParams).toEqual([]);
      dispose();
    });
  });

  it('clearHighlights resets selectedEntity and highlightedParams', () => {
    createRoot((dispose) => {
      const { state, selectEntity, setHighlightedParams, clearHighlights } = createSelectionStore();
      selectEntity('Bracket');
      setHighlightedParams(['c1', 'c2']);
      expect(state.selectedEntity).toBe('Bracket');
      expect(state.highlightedParams).toEqual(['c1', 'c2']);

      clearHighlights();
      expect(state.selectedEntity).toBeNull();
      expect(state.highlightedParams).toEqual([]);
      dispose();
    });
  });

  // --- clearIfRemoved multi-selection (step-15) ---
  describe('clearIfRemoved — multi-selection', () => {
    it('removes only the matching path from selectedEntities, leaving others intact', () => {
      createRoot((dispose) => {
        const { state, toggleSelect, clearIfRemoved } = createSelectionStore();
        toggleSelect('A');
        toggleSelect('B');
        toggleSelect('C');
        expect(state.selectedEntities).toEqual(['A', 'B', 'C']);

        clearIfRemoved('B');
        expect(state.selectedEntities).toEqual(['A', 'C']);
        dispose();
      });
    });

    it('updates selectedEntity to new last element after the removed path was the primary', () => {
      createRoot((dispose) => {
        const { state, toggleSelect, clearIfRemoved } = createSelectionStore();
        toggleSelect('A');
        toggleSelect('B');
        // primary = B (last added)
        expect(state.selectedEntity).toBe('B');

        clearIfRemoved('B');
        // B removed → new last = A
        expect(state.selectedEntity).toBe('A');
        expect(state.selectedEntities).toEqual(['A']);
        dispose();
      });
    });

    it('sets selectedEntity to null when removing the last item in selectedEntities', () => {
      createRoot((dispose) => {
        const { state, selectSingle, clearIfRemoved } = createSelectionStore();
        selectSingle('A');
        clearIfRemoved('A');
        expect(state.selectedEntities).toEqual([]);
        expect(state.selectedEntity).toBeNull();
        dispose();
      });
    });

    it('clears anchorEntity iff it matched the removed path', () => {
      createRoot((dispose) => {
        const { state, selectSingle, toggleSelect, clearIfRemoved } = createSelectionStore();
        selectSingle('A'); // sets anchor = A
        toggleSelect('B'); // adds B; anchor stays A
        expect(state.anchorEntity).toBe('A');

        clearIfRemoved('A'); // A is anchor and was in selectedEntities
        expect(state.anchorEntity).toBeNull();
        dispose();
      });
    });

    it('does NOT clear anchorEntity when the removed path does NOT match it', () => {
      createRoot((dispose) => {
        const { state, selectSingle, toggleSelect, clearIfRemoved } = createSelectionStore();
        selectSingle('A'); // anchor = A
        toggleSelect('B');
        toggleSelect('C');
        expect(state.anchorEntity).toBe('A');

        clearIfRemoved('C'); // remove C; anchor is still A
        expect(state.anchorEntity).toBe('A');
        dispose();
      });
    });

    it('does not affect selectedEntities when path is not present', () => {
      createRoot((dispose) => {
        const { state, toggleSelect, clearIfRemoved } = createSelectionStore();
        toggleSelect('A');
        toggleSelect('B');

        clearIfRemoved('X'); // X not in list
        expect(state.selectedEntities).toEqual(['A', 'B']);
        expect(state.selectedEntity).toBe('B');
        dispose();
      });
    });
  });

  // S8: clearIfRemoved — clears stale references to removed entities
  it('clearIfRemoved clears selectedEntity when it matches the removed path', () => {
    createRoot((dispose) => {
      const { state, selectEntity, clearIfRemoved } = createSelectionStore();
      selectEntity('Bracket.body');
      expect(state.selectedEntity).toBe('Bracket.body');

      clearIfRemoved('Bracket.body');
      expect(state.selectedEntity).toBeNull();
      dispose();
    });
  });

  it('clearIfRemoved clears hoveredEntity when it matches the removed path', () => {
    createRoot((dispose) => {
      const { state, hoverEntity, clearIfRemoved } = createSelectionStore();
      hoverEntity('Bracket.body');
      expect(state.hoveredEntity).toBe('Bracket.body');

      clearIfRemoved('Bracket.body');
      expect(state.hoveredEntity).toBeNull();
      dispose();
    });
  });

  it('clearIfRemoved does NOT clear selection/hover when path does not match', () => {
    createRoot((dispose) => {
      const { state, selectEntity, hoverEntity, clearIfRemoved } = createSelectionStore();
      selectEntity('Bracket.body');
      hoverEntity('Mount.body');

      clearIfRemoved('Other.body');
      expect(state.selectedEntity).toBe('Bracket.body');
      expect(state.hoveredEntity).toBe('Mount.body');
      dispose();
    });
  });

  it('clearIfRemoved clears both selectedEntity and hoveredEntity when both match', () => {
    createRoot((dispose) => {
      const { state, selectEntity, hoverEntity, clearIfRemoved } = createSelectionStore();
      selectEntity('Bracket.body');
      hoverEntity('Bracket.body');

      clearIfRemoved('Bracket.body');
      expect(state.selectedEntity).toBeNull();
      expect(state.hoveredEntity).toBeNull();
      dispose();
    });
  });

  describe('backend sync', () => {
    let dispose!: () => void;
    let selectEntity!: (path: string | null) => void;
    let hoverEntity!: (path: string | null) => void;
    let clearIfRemoved!: (path: string) => void;
    let clearHighlights!: () => void;

    beforeEach(() => {
      vi.useFakeTimers();
      mockInvoke.mockResolvedValue(undefined);

      createRoot((d) => {
        dispose = d;
        const store = createSelectionStore();
        selectEntity = store.selectEntity;
        hoverEntity = store.hoverEntity;
        clearIfRemoved = store.clearIfRemoved;
        clearHighlights = store.clearHighlights;
      });

    });

    afterEach(() => {
      dispose();
      vi.useRealTimers();
      vi.clearAllMocks();
    });

    it('no invoke is dispatched on store creation (no spurious initial sync)', () => {
      // Advance past the debounce window — if a spurious dispatch was
      // scheduled on creation, it would fire here.
      vi.advanceTimersByTime(100);

      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('selection-only change calls invoke immediately (not debounced)', () => {
      selectEntity('Bracket');

      // invoke should have been called synchronously — no timer advancement needed
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: null,
      });
    });

    it('selectEntity(null) dispatches cleared selection to backend immediately', () => {
      selectEntity('Bracket');
      mockInvoke.mockClear();

      selectEntity(null);

      // Selection-only change → immediate dispatch
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });

    it('hover-only change calls invoke only after 100ms debounce', () => {
      hoverEntity('Bracket.width');

      // invoke should NOT have been called yet — it's debounced
      expect(mockInvoke).not.toHaveBeenCalled();

      // Advance past the 100ms debounce
      vi.advanceTimersByTime(100);

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: 'Bracket.width',
      });
    });

    it('combined selection+hover change is debounced at 100ms', () => {
      // Use batch() so both state changes are applied before the effect runs.
      // Without batch(), selectEntity fires one effect (selection-only → immediate)
      // and hoverEntity fires a second effect (hover → debounce), hiding the
      // intermediate immediate call. batch() causes a single effect run where
      // both selectionChanged and hoverChanged are true, hitting the debounce path.
      batch(() => {
        selectEntity('Bracket');
        hoverEntity('Bracket.width');
      });

      // invoke should NOT have been called — both changed, so debounce path
      expect(mockInvoke).not.toHaveBeenCalled();

      // Still nothing at 50ms
      vi.advanceTimersByTime(50);
      expect(mockInvoke).not.toHaveBeenCalled();

      // After 100ms, the combined state is sent
      vi.advanceTimersByTime(50);
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: 'Bracket.width',
      });
    });

    it('immediate selection dispatch cancels pending hover debounce', () => {
      // Start a hover (triggers 100ms debounce)
      hoverEntity('X');
      expect(mockInvoke).not.toHaveBeenCalled();

      // Before the hover timer fires, change selection only
      selectEntity('Bracket');

      // The selection-only change should invoke immediately with the full current state
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: 'X',
      });

      mockInvoke.mockClear();

      // After 100ms, no additional stale invoke should fire
      vi.advanceTimersByTime(100);
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('sequential selectEntity then hoverEntity fires immediate then debounced invoke', () => {
      // Without batch(), each call triggers a separate effect run.
      // selectEntity('Bracket') → selection-only change → immediate invoke
      selectEntity('Bracket');
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: null,
      });

      // hoverEntity('Bracket.width') → hover change → debounce pending
      hoverEntity('Bracket.width');
      expect(mockInvoke).toHaveBeenCalledTimes(1); // still just the one immediate call

      // After 100ms, the debounced hover invoke fires with full current state
      vi.advanceTimersByTime(100);
      expect(mockInvoke).toHaveBeenCalledTimes(2);
      expect(mockInvoke).toHaveBeenLastCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: 'Bracket.width',
      });
    });

    it('rapid hover changes collapse into single invoke at 100ms', () => {
      hoverEntity('A');
      vi.advanceTimersByTime(50);

      // Second hover before the first fires — should reset the debounce
      hoverEntity('B');
      expect(mockInvoke).not.toHaveBeenCalled();

      // 50ms after second hover — still within debounce window
      vi.advanceTimersByTime(50);
      expect(mockInvoke).not.toHaveBeenCalled();

      // 100ms after second hover — should fire with latest value
      vi.advanceTimersByTime(50);
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: 'B',
      });
    });

    it('cleanup disposes the debounce timer', () => {
      hoverEntity('X');
      // Timer is pending
      expect(mockInvoke).not.toHaveBeenCalled();

      // Dispose the root — onCleanup should clear the timer
      dispose();
      // Prevent afterEach from double-disposing
      dispose = () => {};

      // Advance timers well past the debounce window
      vi.advanceTimersByTime(200);
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('invoke rejection is silently caught (no unhandled promise)', async () => {
      mockInvoke.mockRejectedValue(new Error('not in Tauri'));
      const errorSpy = vi.spyOn(console, 'error');

      selectEntity('Bracket');

      // invoke was called (proving dispatch happened)
      expect(mockInvoke).toHaveBeenCalledTimes(1);

      // Flush microtasks so the .catch() handler on the rejected promise executes
      await vi.advanceTimersByTimeAsync(0);

      // No unhandled rejection leaked to console
      expect(errorSpy).not.toHaveBeenCalled();
      errorSpy.mockRestore();
    });

    it('clearIfRemoved dispatches when selectedEntity matches', () => {
      selectEntity('Bracket');
      mockInvoke.mockClear();

      clearIfRemoved('Bracket');

      // Selection-only change → immediate dispatch
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });

    it('clearIfRemoved dispatches when hoveredEntity matches', () => {
      hoverEntity('Bracket.width');
      // Flush the hover debounce so it doesn't interfere
      vi.advanceTimersByTime(100);
      mockInvoke.mockClear();

      clearIfRemoved('Bracket.width');

      // Hover-only change → debounced, not immediate
      expect(mockInvoke).not.toHaveBeenCalled();

      vi.advanceTimersByTime(100);
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });

    it('clearIfRemoved dispatches when both fields match', () => {
      batch(() => {
        selectEntity('X');
        hoverEntity('X');
      });
      // Flush the combined debounced dispatch
      vi.advanceTimersByTime(100);
      mockInvoke.mockClear();

      clearIfRemoved('X');

      // Both fields cleared atomically via batch() — no intermediate dispatch
      expect(mockInvoke).not.toHaveBeenCalled();

      // Single debounced dispatch with both fields null
      vi.advanceTimersByTime(100);
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });

    it('clearIfRemoved does not dispatch when neither field matches', () => {
      batch(() => {
        selectEntity('A');
        hoverEntity('B');
      });
      // Flush the combined debounced dispatch
      vi.advanceTimersByTime(100);
      mockInvoke.mockClear();

      clearIfRemoved('Other');

      // Neither field matched → no state change, no dispatch
      vi.advanceTimersByTime(100);
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('clearHighlights dispatches exactly one backend sync for selection→null', () => {
      // Set up: select an entity (triggers immediate invoke)
      selectEntity('Bracket');
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      mockInvoke.mockClear();

      // Act: clearHighlights uses batch() to set selectedEntity=null and
      // highlightedParams=[] atomically. The sync effect only tracks
      // selectedEntity/hoveredEntity, so the invoke count is 1 regardless,
      // but batch() prevents intermediate state from being visible to other
      // reactive subscribers.
      clearHighlights();

      // Advance past any pending debounce
      vi.advanceTimersByTime(100);

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });
  });

  // --- Backend sync: selectedEntities forwarded to IPC (step-17) ---
  describe('backend sync — multi-selection IPC payload', () => {
    let dispose!: () => void;
    let selectSingle!: (path: string | null) => void;
    let toggleSelect!: (path: string) => void;
    let clearSelection!: () => void;
    let hoverEntity!: (path: string | null) => void;

    beforeEach(() => {
      vi.useFakeTimers();
      mockInvoke.mockResolvedValue(undefined);

      createRoot((d) => {
        dispose = d;
        const store = createSelectionStore();
        selectSingle = store.selectSingle;
        toggleSelect = store.toggleSelect;
        clearSelection = store.clearSelection;
        hoverEntity = store.hoverEntity;
      });
    });

    afterEach(() => {
      dispose();
      vi.useRealTimers();
      vi.clearAllMocks();
    });

    it('selectSingle dispatches invoke with selectedEntities list', () => {
      selectSingle('Bracket');

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
        selectedEntities: ['Bracket'],
        hoveredEntity: null,
      });
    });

    it('toggleSelect dispatches invoke with updated selectedEntities list', () => {
      selectSingle('A');
      mockInvoke.mockClear();

      toggleSelect('B');

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'B',
        selectedEntities: ['A', 'B'],
        hoveredEntity: null,
      });
    });

    it('clearSelection dispatches invoke with empty selectedEntities list', () => {
      selectSingle('A');
      mockInvoke.mockClear();

      clearSelection();

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: null,
        selectedEntities: [],
        hoveredEntity: null,
      });
    });

    it('hover-only change sends current selectedEntities snapshot after debounce', () => {
      selectSingle('A');
      mockInvoke.mockClear();

      hoverEntity('A.width');

      // Should be debounced
      expect(mockInvoke).not.toHaveBeenCalled();

      vi.advanceTimersByTime(100);

      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'A',
        selectedEntities: ['A'],
        hoveredEntity: 'A.width',
      });
    });
  });
});
