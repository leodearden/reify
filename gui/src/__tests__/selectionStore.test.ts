import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';

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

  it('selectEntity sets selectedEntity', () => {
    createRoot((dispose) => {
      const { state, selectEntity } = createSelectionStore();
      selectEntity('Bracket');
      expect(state.selectedEntity).toBe('Bracket');
      dispose();
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

    beforeEach(() => {
      vi.useFakeTimers();
      mockInvoke.mockResolvedValue(undefined);

      createRoot((d) => {
        dispose = d;
        const store = createSelectionStore();
        selectEntity = store.selectEntity;
        hoverEntity = store.hoverEntity;
      });

      // Flush the initial effect's debounced invoke (null, null)
      vi.advanceTimersByTime(100);
      mockInvoke.mockClear();
    });

    afterEach(() => {
      dispose();
      vi.useRealTimers();
      vi.clearAllMocks();
    });

    it('selection-only change calls invoke immediately (not debounced)', () => {
      selectEntity('Bracket');

      // invoke should have been called synchronously — no timer advancement needed
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
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
        hoveredEntity: 'Bracket.width',
      });
    });

    it('combined selection+hover change is debounced at 100ms', () => {
      selectEntity('Bracket');
      hoverEntity('Bracket.width');

      // With hover change in the mix, everything should be debounced
      // (selectEntity fires effect, then hoverEntity fires another effect that replaces the timer)
      // After both, the latest state is not yet sent
      mockInvoke.mockClear();

      // Nothing sent yet (debounce pending)
      vi.advanceTimersByTime(50);
      expect(mockInvoke).not.toHaveBeenCalled();

      // After 100ms total, the combined state is sent
      vi.advanceTimersByTime(50);
      expect(mockInvoke).toHaveBeenCalledTimes(1);
      expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
        selectedEntity: 'Bracket',
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
        hoveredEntity: 'X',
      });

      mockInvoke.mockClear();

      // After 100ms, no additional stale invoke should fire
      vi.advanceTimersByTime(100);
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });
});
