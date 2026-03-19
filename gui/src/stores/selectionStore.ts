import { createStore } from 'solid-js/store';

export interface SelectionState {
  selectedEntity: string | null;
  hoveredEntity: string | null;
}

export function createSelectionStore() {
  const [state, setState] = createStore<SelectionState>({
    selectedEntity: null,
    hoveredEntity: null,
  });

  function selectEntity(entityPath: string | null) {
    setState('selectedEntity', entityPath);
  }

  function hoverEntity(entityPath: string | null) {
    setState('hoveredEntity', entityPath);
  }

  return { state, selectEntity, hoverEntity };
}
