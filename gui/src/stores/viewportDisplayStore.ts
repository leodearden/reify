/**
 * The global, persisted viewport display preferences (edges-in-black view, alpha-spine D5).
 */
import { createStore } from 'solid-js/store';

export const VIEWPORT_DISPLAY_STORAGE_KEY = 'reify-viewport-display';

export interface ViewportDisplayState {
  edges: boolean;
}

const DEFAULT_VIEWPORT_DISPLAY: Readonly<ViewportDisplayState> = { edges: false };

/** Read the persisted preferences; anything missing, corrupt or mistyped reads as the default. */
export function loadViewportDisplay(): ViewportDisplayState {
  try {
    const raw = localStorage.getItem(VIEWPORT_DISPLAY_STORAGE_KEY);
    if (raw === null) return { ...DEFAULT_VIEWPORT_DISPLAY };
    const edges = (JSON.parse(raw) as { edges?: unknown } | null)?.edges;
    if (typeof edges === 'boolean') return { edges };
  } catch {
    // Corrupt JSON or unavailable storage: fall through to the default.
  }
  return { ...DEFAULT_VIEWPORT_DISPLAY };
}

export function createViewportDisplayStore() {
  const [state, setState] = createStore<ViewportDisplayState>(loadViewportDisplay());

  function setEdges(on: boolean): void {
    setState('edges', on);
    try {
      localStorage.setItem(VIEWPORT_DISPLAY_STORAGE_KEY, JSON.stringify({ edges: state.edges }));
    } catch {
      // Silently ignore — localStorage may be full or unavailable.
    }
  }

  function toggleEdges(): void {
    setEdges(!state.edges);
  }

  function reset(): void {
    setEdges(DEFAULT_VIEWPORT_DISPLAY.edges);
  }

  return { state, setEdges, toggleEdges, reset };
}

export type ViewportDisplayStore = ReturnType<typeof createViewportDisplayStore>;
