import { batch, createEffect, onCleanup } from 'solid-js';
import { createStore } from 'solid-js/store';
import { invoke } from '@tauri-apps/api/core';

export interface SelectionState {
  selectedEntity: string | null;
  selectedEntities: string[];
  anchorEntity: string | null;
  hoveredEntity: string | null;
  highlightedParams: string[];
}

export function createSelectionStore() {
  const [state, setState] = createStore<SelectionState>({
    selectedEntity: null,
    selectedEntities: [],
    anchorEntity: null,
    hoveredEntity: null,
    highlightedParams: [],
  });

  // Sync selection state to the Rust backend so MCP tools can read it.
  // Two dispatch paths:
  //   - Selection-only changes (clicks): sent immediately since they are infrequent
  //     and MCP tools may read selection in the same interaction.
  //   - Hover changes (with or without selection): debounced at 100ms to avoid
  //     flooding the backend during mouse movement.
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let prevSelected: string | null = null;
  let prevHovered: string | null = null;

  const sendSelection = (selected: string | null, hovered: string | null) => {
    invoke('update_selection', {
      selectedEntity: selected,
      hoveredEntity: hovered,
    }).catch(() => {
      // Ignore errors (e.g. when running outside Tauri in tests)
    });
  };

  createEffect(() => {
    const selected = state.selectedEntity;
    const hovered = state.hoveredEntity;

    // Always clear any pending debounce to avoid sending stale state
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }

    const selectionChanged = selected !== prevSelected;
    const hoverChanged = hovered !== prevHovered;

    prevSelected = selected;
    prevHovered = hovered;

    if (!selectionChanged && !hoverChanged) return;

    if (selectionChanged && !hoverChanged) {
      // Selection-only change — dispatch immediately
      sendSelection(selected, hovered);
    } else {
      // Hover changed (possibly with selection) — debounce at 100ms
      hoverTimer = setTimeout(() => {
        hoverTimer = null;
        sendSelection(selected, hovered);
      }, 100);
    }
  });

  onCleanup(() => {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
    }
  });

  function selectSingle(entityPath: string | null) {
    if (entityPath === null) {
      batch(() => {
        setState('selectedEntities', []);
        setState('selectedEntity', null);
        setState('anchorEntity', null);
      });
    } else {
      batch(() => {
        setState('selectedEntities', [entityPath]);
        setState('selectedEntity', entityPath);
        setState('anchorEntity', entityPath);
      });
    }
  }

  function rangeSelect(paths: string[]) {
    const deduped = Array.from(new Set(paths));
    batch(() => {
      setState('selectedEntities', deduped);
      setState('selectedEntity', deduped.length > 0 ? deduped[deduped.length - 1] : null);
    });
  }

  function toggleSelect(entityPath: string) {
    batch(() => {
      const current = state.selectedEntities;
      const idx = current.indexOf(entityPath);
      let next: string[];
      if (idx >= 0) {
        next = current.filter((p) => p !== entityPath);
      } else {
        next = [...current, entityPath];
      }
      setState('selectedEntities', next);
      setState('selectedEntity', next.length > 0 ? next[next.length - 1] : null);
    });
  }

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
    batch(() => {
      setState('selectedEntity', null);
      setState('highlightedParams', []);
    });
  }

  function clearIfRemoved(entityPath: string) {
    batch(() => {
      if (state.selectedEntity === entityPath) {
        selectEntity(null);
      }
      if (state.hoveredEntity === entityPath) {
        hoverEntity(null);
      }
    });
  }

  return { state, selectSingle, toggleSelect, rangeSelect, selectEntity, hoverEntity, setHighlightedParams, clearHighlights, clearIfRemoved };
}
