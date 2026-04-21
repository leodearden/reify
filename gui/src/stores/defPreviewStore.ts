import { createStore } from 'solid-js/store';
import type { MeshData, GuiState } from '../types';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface DefPreviewState {
  /** Name of the definition whose preview meshes are currently loaded, or null. */
  defName: string | null;
  /** Preview meshes keyed by entity_path (mirrors engineStore.state.meshes shape). */
  meshes: Record<string, MeshData>;
  /** True while a fetch is in flight. */
  isLoading: boolean;
  /** Last error message, or null if no error. */
  error: string | null;
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

export function createDefPreviewStore() {
  const [state, setState] = createStore<DefPreviewState>({
    defName: null,
    meshes: {},
    isLoading: false,
    error: null,
  });

  /** Apply a fetched GuiState as the current preview. Keys meshes by entity_path. */
  function applyPreview(defName: string, guiState: GuiState): void {
    const meshes: Record<string, MeshData> = {};
    for (const m of guiState.meshes) {
      meshes[m.entity_path] = m;
    }
    setState({ defName, meshes, isLoading: false, error: null });
  }

  /** Reset the store to its initial empty state. */
  function clearPreview(): void {
    setState({ defName: null, meshes: {}, isLoading: false, error: null });
  }

  /** Record an error and clear the loading flag. */
  function setError(message: string): void {
    setState({ error: message, isLoading: false });
  }

  /** Set the loading flag (internal use, exposed for testing). */
  function setLoading(loading: boolean): void {
    setState('isLoading', loading);
  }

  return { state, applyPreview, clearPreview, setError, setLoading };
}

export type DefPreviewStore = ReturnType<typeof createDefPreviewStore>;
