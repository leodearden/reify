import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createSelectionStore } from '../stores/selectionStore';

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
});
