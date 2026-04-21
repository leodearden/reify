import { createStore, produce } from 'solid-js/store';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/** Camera state serialised as plain arrays — Three.js-independent and persistable. */
export interface CameraState {
  position: [number, number, number];
  target: [number, number, number];
  up: [number, number, number];
  zoom: number;
}

/** Per-viewport state. */
export interface ViewportState {
  id: string;
  /** Viewport type: "design" for the main design canvas, "def-preview" for the definition preview. */
  type: 'design' | 'def-preview';
  /** The currently assigned view id (from viewStateStore), or null if none assigned. */
  viewId: string | null;
  /** Path of the definition being previewed (def-preview type only), or null. */
  defPath: string | null;
  /** Whether this viewport is the active/focused viewport. */
  active: boolean;
  /** Persisted camera state. */
  camera: CameraState;
}

/** Top-level store state shape. */
export interface ViewportStoreState {
  viewports: Record<string, ViewportState>;
}

// ---------------------------------------------------------------------------
// Default camera state
// ---------------------------------------------------------------------------

const DEFAULT_CAMERA: CameraState = {
  position: [5, 5, 5],
  target: [0, 0, 0],
  up: [0, 1, 0],
  zoom: 1,
};

// ---------------------------------------------------------------------------
// Default viewport layout
// ---------------------------------------------------------------------------

/**
 * The default two-viewport layout seeded into a fresh store (design-main +
 * def-preview). Exported so tests and call sites can extend or override it
 * without importing the raw constants.
 */
export const DEFAULT_VIEWPORTS: Record<string, ViewportState> = {
  'design-main': {
    id: 'design-main',
    type: 'design',
    viewId: null,
    defPath: null,
    active: true,
    camera: { ...DEFAULT_CAMERA },
  },
  'def-preview': {
    id: 'def-preview',
    type: 'def-preview',
    viewId: null,
    defPath: null,
    active: false,
    camera: { ...DEFAULT_CAMERA },
  },
};

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

/**
 * Create a viewport store.
 *
 * @param initialViewports - Optional override for the initial `viewports` map.
 *   Defaults to `DEFAULT_VIEWPORTS` (design-main + def-preview). Pass a
 *   custom map to construct minimal stores in tests or to support alternative
 *   layouts (single-viewport embedded view, four-up, etc.).
 */
export function createViewportStore(
  initialViewports?: Record<string, ViewportState>,
) {
  const [state, setState] = createStore<ViewportStoreState>({
    viewports: initialViewports ?? DEFAULT_VIEWPORTS,
  });

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  function getViewport(id: string): ViewportState | undefined {
    return state.viewports[id];
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  /**
   * Set the active viewport by id. All other viewports have `active` set to
   * false. Returns `false` if the id is not found (no mutation); `true` on
   * success.
   */
  function setActiveViewport(id: string): boolean {
    if (!state.viewports[id]) return false;
    setState(
      produce((s) => {
        for (const key of Object.keys(s.viewports)) {
          s.viewports[key].active = key === id;
        }
      }),
    );
    return true;
  }

  /**
   * Assign a view id to a viewport. Returns `false` if the viewport is not
   * found; `true` on success. Passing `null` clears the assignment.
   */
  function assignView(viewportId: string, viewId: string | null): boolean {
    if (!state.viewports[viewportId]) return false;
    setState('viewports', viewportId, 'viewId', viewId);
    return true;
  }

  /**
   * Persist camera state for a viewport. Returns `false` if the viewport is
   * not found; `true` on success.
   */
  function updateCamera(viewportId: string, camera: CameraState): boolean {
    if (!state.viewports[viewportId]) return false;
    setState('viewports', viewportId, 'camera', { ...camera });
    return true;
  }

  return {
    state,
    getViewport,
    setActiveViewport,
    assignView,
    updateCamera,
  };
}

export type ViewportStore = ReturnType<typeof createViewportStore>;
