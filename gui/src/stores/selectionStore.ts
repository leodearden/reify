import { createStore } from 'solid-js/store';

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

  return { state, selectEntity, hoverEntity, setHighlightedParams, clearHighlights };
}
