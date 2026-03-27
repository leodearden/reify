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
    beforeEach(() => {
      vi.useFakeTimers();
      mockInvoke.mockResolvedValue(undefined);
    });

    afterEach(() => {
      vi.useRealTimers();
      vi.clearAllMocks();
    });

    it('selection-only change calls invoke immediately (not debounced)', () => {
      createRoot((dispose) => {
        const { selectEntity } = createSelectionStore();

        // Clear the initial effect invocation (both null)
        mockInvoke.mockClear();

        selectEntity('Bracket');

        // invoke should have been called synchronously — no timer advancement needed
        expect(mockInvoke).toHaveBeenCalledTimes(1);
        expect(mockInvoke).toHaveBeenCalledWith('update_selection', {
          selectedEntity: 'Bracket',
          hoveredEntity: null,
        });

        dispose();
      });
    });
  });
});
