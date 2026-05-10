import { createStore } from 'solid-js/store';
import type { Palette, Range } from '../viewport/colormap';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/** State shape for the FEA-mode store. */
export interface FeaModeState {
  /** Whether FEA colorization is currently active. */
  enabled: boolean;
  /** The scalar channel to visualise (e.g. 'vonMises', 'displacement_magnitude'). */
  channel: string;
  /** The colormap palette to apply. */
  palette: Palette;
  /** The scalar range (mode + bounds). */
  range: Range;
  /**
   * Sticky flag — set to true on the first auto-enable.
   * Once set, tryAutoEnable() is a no-op so user toggles are not overridden.
   */
  autoEnabledOnce: boolean;
  /**
   * Whether the deformed-shape view is active.
   * When true, meshManager.setDeformation({ warpFactor }) is called by the
   * Viewport bridge effect; when false, setDeformation(null) is called.
   * Default: false.
   */
  showDeformed: boolean;
  /**
   * Scale factor applied to the displacement delta.
   * position[i] = vertices[i] + warpFactor * (displaced[i] - vertices[i])
   * 1.0 = true-scale deformation; 10.0 / 100.0 = amplified for small displacements.
   * Default: 1.0. Non-finite values are rejected by setWarpFactor.
   */
  warpFactor: number;
}

/** Return type of createFeaModeStore(). */
export interface FeaModeStore {
  state: FeaModeState;
  setEnabled(b: boolean): void;
  setChannel(c: string): void;
  setPalette(p: Palette): void;
  /** Returns false (no-op) if min or max is not finite. */
  setRange(r: Range): boolean;
  /** Lock range to explicit bounds with a provenance label. Returns false if either bound is non-finite. */
  lockCurrent(min: number, max: number, source?: string): boolean;
  /**
   * Auto-enable once (one-shot). If `autoEnabledOnce` is already true, returns false
   * and does nothing — ensures user disable is sticky.
   */
  tryAutoEnable(channel?: string): boolean;
  /** Toggle the deformed-shape view on/off. */
  setShowDeformed(b: boolean): void;
  /**
   * Set the warp factor for deformed-shape rendering.
   * Returns false and is a no-op when `f` is NaN or ±Infinity.
   * Mirrors the non-finite guard in setRange.
   */
  setWarpFactor(f: number): boolean;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

const DEFAULT_RANGE: Range = { mode: 'auto', min: 0, max: 1 };

/**
 * Create the FEA-mode store.
 *
 * Returns a reactive `state` object backed by a SolidJS createStore.
 * Mutations are added incrementally in subsequent steps.
 */
export function createFeaModeStore(): FeaModeStore {
  const [state, setState] = createStore<FeaModeState>({
    enabled: false,
    channel: 'vonMises',
    palette: 'viridis',
    range: { ...DEFAULT_RANGE },
    autoEnabledOnce: false,
    showDeformed: false,
    warpFactor: 1.0,
  });

  function setEnabled(b: boolean): void {
    setState('enabled', b);
  }

  function setChannel(c: string): void {
    setState('channel', c);
  }

  function setPalette(p: Palette): void {
    setState('palette', p);
  }

  function setRange(r: Range): boolean {
    if (!Number.isFinite(r.min) || !Number.isFinite(r.max)) {
      return false;
    }
    setState('range', { ...r });
    return true;
  }

  function lockCurrent(min: number, max: number, source = 'current'): boolean {
    if (!Number.isFinite(min) || !Number.isFinite(max)) {
      return false;
    }
    setState('range', { mode: 'locked', min, max, source });
    return true;
  }

  function setShowDeformed(b: boolean): void {
    setState('showDeformed', b);
  }

  function setWarpFactor(f: number): boolean {
    if (!Number.isFinite(f)) return false;
    setState('warpFactor', f);
    return true;
  }

  function tryAutoEnable(channel?: string): boolean {
    if (state.autoEnabledOnce) {
      return false;
    }
    setState('autoEnabledOnce', true);
    setState('enabled', true);
    if (channel !== undefined) {
      setState('channel', channel);
    }
    return true;
  }

  return { state, setEnabled, setChannel, setPalette, setRange, lockCurrent, tryAutoEnable, setShowDeformed, setWarpFactor };
}
