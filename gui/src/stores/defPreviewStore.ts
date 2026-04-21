import { createStore } from 'solid-js/store';
import type { MeshData, GuiState } from '../types';
import { errorMessage } from '../utils/errorClassifier';

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

  /**
   * Monotonically-increasing token for race-condition guarding inside loadPreview.
   * Each loadPreview call increments this and captures its value; after the await,
   * a mismatch means a newer call has superseded the current one.
   */
  let latestLoadToken = 0;

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

  /**
   * Fetch and apply preview meshes for a definition.
   *
   * De-duplication: early-returns (no-op) if `defName` matches the currently-loaded
   * `state.defName`. To force a re-fetch of the same definition (e.g. after an upstream
   * source change), callers must invoke `clearPreview()` first to reset `state.defName`.
   *
   * Sets `isLoading=true` synchronously, then on resolve calls `applyPreview`,
   * on reject calls `setError`.
   *
   * Race-condition guard: each call captures a monotonically-increasing `token`
   * before awaiting `fetch`. After the await, `if (token !== latestLoadToken)`
   * the result is stale (a newer call has superseded this one) and is silently
   * discarded. `isLoading` is reset inside the guarded finally path so a stale
   * in-flight load cannot flip `isLoading` false and make a fresh load look done.
   *
   * The `fetch` callback is always invoked via `Promise.resolve().then(...)` so any
   * synchronous throw from `fetch` is routed through the catch branch rather than
   * propagating synchronously and leaking the `setLoading(true)` call.
   */
  async function loadPreview(
    defName: string,
    fetch: (name: string) => Promise<GuiState>,
  ): Promise<void> {
    // De-duplication: skip if the same definition is already loaded
    if (state.defName === defName) return;

    // Capture token before the await (post same-defName guard)
    const token = ++latestLoadToken;
    setLoading(true);
    try {
      // Wrap in Promise.resolve().then() so any synchronous throw from `fetch`
      // is captured by the catch branch rather than escaping past setLoading(true).
      const guiState = await Promise.resolve().then(() => fetch(defName));
      // Discard stale results: a newer loadPreview call has superseded this one
      if (token !== latestLoadToken) return;
      applyPreview(defName, guiState);
    } catch (err) {
      // Discard stale errors too
      if (token !== latestLoadToken) return;
      setError(errorMessage(err));
    }
  }

  return { state, applyPreview, clearPreview, setError, setLoading, loadPreview };
}

export type DefPreviewStore = ReturnType<typeof createDefPreviewStore>;
