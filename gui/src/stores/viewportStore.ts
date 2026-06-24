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
  /** Viewport type: "design" for the main design canvas, "def-preview" for the definition preview,
   *  or "pane" for a dynamically-added model pane (index >= 1). */
  type: 'design' | 'def-preview' | 'pane';
  /** The currently assigned view id (from viewStateStore), or null if none assigned. */
  viewId: string | null;
  /** Path of the definition being previewed (def-preview type only), or null. */
  defPath: string | null;
  /** Whether this viewport is the active/focused viewport. */
  active: boolean;
  /** User override: force this viewport expanded even when auto-activation says it should be minimized. */
  forceExpanded: boolean;
  /** Persisted camera state. */
  camera: CameraState;
  /** Model pane index (only set for type === 'pane'; pane-0 aliases design-main). */
  paneIndex?: number;
  /** Per-pane size weight for the N-pane grid layout (default 1). Replaces the scalar
   *  splitRatio in the β MultiViewport rewrite; until then both fields coexist. */
  sizeWeight: number;
}

/** Top-level store state shape. */
export interface ViewportStoreState {
  viewports: Record<string, ViewportState>;
  /** Fraction of container height allocated to the def-preview viewport when both are active. Clamped to [0.1, 0.9]. */
  splitRatio: number;
}

// ---------------------------------------------------------------------------
// Default camera state (private — read-only; never mutated directly)
// ---------------------------------------------------------------------------

const DEFAULT_CAMERA: CameraState = {
  position: [5, 5, 5],
  target: [0, 0, 0],
  up: [0, 0, 1],
  zoom: 1,
};

/** Return a deep clone of a CameraState so tuple arrays are distinct references. */
function cloneCamera(c: CameraState): CameraState {
  return {
    position: [...c.position] as [number, number, number],
    target: [...c.target] as [number, number, number],
    up: [...c.up] as [number, number, number],
    zoom: c.zoom,
  };
}

// ---------------------------------------------------------------------------
// Default viewport layout factory
// ---------------------------------------------------------------------------

/**
 * Build a fresh default two-viewport layout (design-main + def-preview).
 * Returns a new object every call so stores never share mutable references.
 */
function defaultViewports(): Record<string, ViewportState> {
  return {
    'design-main': {
      id: 'design-main',
      type: 'design',
      viewId: null,
      defPath: null,
      active: true,
      forceExpanded: false,
      camera: cloneCamera(DEFAULT_CAMERA),
      sizeWeight: 1,
    },
    'def-preview': {
      id: 'def-preview',
      type: 'def-preview',
      viewId: null,
      defPath: null,
      active: false,
      forceExpanded: false,
      camera: cloneCamera(DEFAULT_CAMERA),
      sizeWeight: 1,
    },
  };
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

/**
 * Create a viewport store.
 *
 * @param initialViewports - Optional override for the initial `viewports` map.
 *   Defaults to a freshly-cloned two-viewport layout (design-main + def-preview).
 *   Pass a custom map to construct minimal stores in tests or to support
 *   alternative layouts (single-viewport embedded view, four-up, etc.).
 */
export function createViewportStore(
  initialViewports?: Record<string, ViewportState>,
) {
  const [state, setState] = createStore<ViewportStoreState>({
    viewports: initialViewports ?? defaultViewports(),
    splitRatio: 0.5,
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

  /**
   * Set the defPath for a viewport. Returns `false` if the viewport is
   * not found; `true` on success. Passing `null` clears the defPath.
   */
  function setDefPath(viewportId: string, defPath: string | null): boolean {
    if (!state.viewports[viewportId]) return false;
    setState('viewports', viewportId, 'defPath', defPath);
    return true;
  }

  /**
   * Set the forceExpanded override for a viewport. Returns `false` if the
   * viewport is not found; `true` on success.
   */
  function setForceExpanded(viewportId: string, flag: boolean): boolean {
    if (!state.viewports[viewportId]) return false;
    setState('viewports', viewportId, 'forceExpanded', flag);
    return true;
  }

  /**
   * Set the split ratio (fraction of container height for the def-preview viewport).
   * Clamps the value to [0.1, 0.9] so neither viewport collapses to zero.
   * Returns `false` when `ratio` is not a finite number (NaN, ±Infinity) — no
   * mutation occurs in that case, preventing NaN from corrupting the layout.
   * Returns `true` on success.
   */
  function setSplitRatio(ratio: number): boolean {
    if (!Number.isFinite(ratio)) return false;
    const clamped = Math.min(0.9, Math.max(0.1, ratio));
    setState('splitRatio', clamped);
    return true;
  }

  /**
   * Add a model pane viewport by pane index.
   * - paneIndex === 0: alias for 'design-main' — returns 'design-main' with NO mutation.
   * - paneIndex >= 1: creates viewport with id `pane-{k}` (idempotent — re-adding an
   *   existing index returns the existing id without mutating the map).
   * Returns the viewport id.
   */
  function addPane(paneIndex: number): string {
    if (paneIndex === 0) return 'design-main';
    const id = `pane-${paneIndex}`;
    if (state.viewports[id]) return id;
    setState(
      produce((s) => {
        s.viewports[id] = {
          id,
          type: 'pane',
          viewId: null,
          defPath: null,
          active: false,
          forceExpanded: false,
          paneIndex,
          camera: cloneCamera(DEFAULT_CAMERA),
          sizeWeight: 1,
        };
      }),
    );
    return id;
  }

  /**
   * Remove a 'pane'-type viewport by pane index.
   * - paneIndex < 1: returns false (pane-0 alias is protected; design-main/def-preview are never removed).
   * - paneIndex >= 1: looks up `pane-{k}`; returns false if absent or not type 'pane'.
   *   Otherwise deletes the entry and returns true.
   */
  function removePane(paneIndex: number): boolean {
    if (paneIndex < 1) return false;
    const id = `pane-${paneIndex}`;
    const vp = state.viewports[id];
    if (!vp || vp.type !== 'pane') return false;
    setState(
      produce((s) => {
        delete s.viewports[id];
      }),
    );
    return true;
  }

  /**
   * Set the per-pane size weight for any viewport.
   * Rejects non-finite values (NaN, ±Infinity) and non-positive values (zero or negative
   * would collapse the pane to zero height/width in the layout grid).
   * Returns `false` when the viewport is not found or weight is invalid; `true` on success.
   */
  function setSizeWeight(viewportId: string, weight: number): boolean {
    if (!state.viewports[viewportId]) return false;
    if (!Number.isFinite(weight) || weight <= 0) return false;
    setState('viewports', viewportId, 'sizeWeight', weight);
    return true;
  }

  return {
    state,
    getViewport,
    setActiveViewport,
    assignView,
    updateCamera,
    setDefPath,
    setForceExpanded,
    setSplitRatio,
    addPane,
    removePane,
    setSizeWeight,
  };
}

export type ViewportStore = ReturnType<typeof createViewportStore>;
