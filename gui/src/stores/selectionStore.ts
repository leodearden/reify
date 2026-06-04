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
  let prevSelectedEntities: string[] = [];
  let prevHovered: string | null = null;

  function entitiesEqual(a: string[], b: string[]): boolean {
    return a.length === b.length && a.every((v, i) => v === b[i]);
  }

  const sendSelection = (selected: string | null, entities: string[], hovered: string | null) => {
    invoke('update_selection', {
      selectedEntity: selected,
      selectedEntities: entities,
      hoveredEntity: hovered,
    }).catch(() => {
      // Ignore errors (e.g. when running outside Tauri in tests)
    });
  };

  createEffect(() => {
    const selected = state.selectedEntity;
    const entities = state.selectedEntities; // reactive dependency on the list
    const hovered = state.hoveredEntity;

    // Always clear any pending debounce to avoid sending stale state
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }

    // Selection changed if the primary OR the list contents differ
    const selectionChanged =
      selected !== prevSelected || !entitiesEqual(entities, prevSelectedEntities);
    const hoverChanged = hovered !== prevHovered;

    prevSelected = selected;
    prevSelectedEntities = entities.slice(); // fresh copy for next comparison
    prevHovered = hovered;

    if (!selectionChanged && !hoverChanged) return;

    if (selectionChanged && !hoverChanged) {
      // Selection-only change — dispatch immediately
      sendSelection(selected, entities.slice(), hovered);
    } else {
      // Hover changed (possibly with selection) — debounce at 100ms
      const entitiesSnapshot = entities.slice();
      hoverTimer = setTimeout(() => {
        hoverTimer = null;
        sendSelection(selected, entitiesSnapshot, hovered);
      }, 100);
    }
  });

  onCleanup(() => {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
    }
  });

  /**
   * Shared mutation helper: replaces selectedEntities with a deduped copy of `paths`
   * and sets selectedEntity to the last element (or null if empty).
   * Does NOT touch anchorEntity — callers manage anchor independently.
   * Clears highlightedParams: any user-gesture selection drops stale constraint highlights.
   */
  function setEntitiesList(paths: string[]): void {
    const deduped = Array.from(new Set(paths));
    batch(() => {
      setState('selectedEntities', deduped);
      setState('selectedEntity', deduped.length > 0 ? deduped[deduped.length - 1] : null);
      setState('highlightedParams', []);
    });
  }

  /**
   * Selects a single entity, replacing any previous selection.
   * Anchor policy: reset to `path` (or cleared if path is null), making this entity
   * the new anchor for subsequent Shift+click range-selects.
   */
  function selectSingle(entityPath: string | null) {
    if (entityPath === null) {
      batch(() => {
        setState('selectedEntities', []);
        setState('selectedEntity', null);
        setState('anchorEntity', null);
        setState('highlightedParams', []);
      });
    } else {
      batch(() => {
        setState('selectedEntities', [entityPath]);
        setState('selectedEntity', entityPath);
        setState('anchorEntity', entityPath);
        setState('highlightedParams', []);
      });
    }
  }

  /**
   * Clears the entire selection.
   * Anchor policy: cleared (null), since there is no longer a logical range origin.
   */
  function clearSelection() {
    batch(() => {
      setState('selectedEntities', []);
      setState('selectedEntity', null);
      setState('anchorEntity', null);
      setState('highlightedParams', []);
    });
  }

  /**
   * Replaces the selection with the given ordered list of paths (e.g. a Shift+click range).
   * Anchor policy: preserved — callers set the anchor before invoking, so subsequent
   * Shift+clicks extend from the same original anchor (standard file-explorer semantics).
   */
  function rangeSelect(paths: string[]) {
    setEntitiesList(paths);
  }

  /**
   * Replaces the selection with all provided paths (e.g. Ctrl+A select-all).
   * Anchor policy: preserved — since Ctrl+A selects everything, the anchor position
   * doesn't meaningfully affect subsequent Shift+clicks.
   */
  function selectAll(paths: string[]) {
    setEntitiesList(paths);
  }

  /**
   * Toggles `entityPath` in the selection: adds it if absent, removes it if present.
   * Anchor policy: NOT updated — the anchor retains its prior value so Shift+click can
   * still range-extend from the last selectSingle call. If no anchor has been set yet,
   * subsequent Shift+clicks fall back to onSelect(path, { shift: true }).
   */
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
      setState('highlightedParams', []);
    });
  }

  // Backward-compat alias: selectEntity(path) === selectSingle(path),
  // selectEntity(null) === clearSelection().  All existing call sites
  // (App.tsx, navigation.ts, debug bridge) continue to work unchanged.
  function selectEntity(entityPath: string | null) {
    if (entityPath === null) {
      clearSelection();
    } else {
      selectSingle(entityPath);
    }
  }

  function hoverEntity(entityPath: string | null) {
    setState('hoveredEntity', entityPath);
  }

  function setHighlightedParams(ids: string[]) {
    setState('highlightedParams', ids);
  }

  function clearIfRemoved(entityPath: string) {
    batch(() => {
      // Remove from multi-selection list and recompute primary
      const nextEntities = state.selectedEntities.filter((p) => p !== entityPath);
      if (nextEntities.length !== state.selectedEntities.length) {
        // The path was present — update list and recompute primary
        setState('selectedEntities', nextEntities);
        setState('selectedEntity', nextEntities.length > 0 ? nextEntities[nextEntities.length - 1] : null);
      }
      // Clear anchor only when it matches the removed path
      if (state.anchorEntity === entityPath) {
        setState('anchorEntity', null);
      }
      // Clear hover when it matches
      if (state.hoveredEntity === entityPath) {
        hoverEntity(null);
      }
    });
  }

  return { state, selectSingle, toggleSelect, rangeSelect, selectAll, clearSelection, selectEntity, hoverEntity, setHighlightedParams, clearIfRemoved };
}
