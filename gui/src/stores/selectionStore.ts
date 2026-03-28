import { createEffect, onCleanup } from 'solid-js';
import { createStore } from 'solid-js/store';
import { invoke } from '@tauri-apps/api/core';

export interface SelectionState {
  selectedEntity: string | null;
  hoveredEntity: string | null;
  highlightedParams: string[];
}

export function createSelectionStore() {
  const [state, setState] = createStore<SelectionState>({
    selectedEntity: null,
    hoveredEntity: null,
    highlightedParams: [],
  });

  // Sync selection state to the Rust backend so MCP tools can read it.
  // Hover updates are debounced at 100ms to avoid flooding the backend;
  // selection (click) updates are sent immediately since they are infrequent.
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;

  createEffect(() => {
    const selected = state.selectedEntity;
    const hovered = state.hoveredEntity;

    // Clear any pending debounce so we always send the latest state
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }

    // Debounce hover updates (100ms), but send immediately if only selection changed
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      invoke('update_selection', {
        selectedEntity: selected,
        hoveredEntity: hovered,
      }).catch(() => {
        // Ignore errors (e.g. when running outside Tauri in tests)
      });
    }, 100);
  });

  onCleanup(() => {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
    }
  });

  function selectEntity(entityPath: string | null) {
    setState('selectedEntity', entityPath);
  }

  function hoverEntity(entityPath: string | null) {
    setState('hoveredEntity', entityPath);
  }

  function setHighlightedParams(ids: string[]) {
    setState('highlightedParams', ids);
  }

  function clearHighlights() {
    setState('selectedEntity', null);
    setState('highlightedParams', []);
  }

  function clearIfRemoved(entityPath: string) {
    if (state.selectedEntity === entityPath) {
      selectEntity(null);
    }
    if (state.hoveredEntity === entityPath) {
      hoverEntity(null);
    }
  }

  return { state, selectEntity, hoverEntity, setHighlightedParams, clearHighlights, clearIfRemoved };
}
