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
}

/** Alias for the store state type (matches existing store naming convention). */
export type FeaModeStoreState = FeaModeState;

/** Return type of createFeaModeStore(). */
export interface FeaModeStore {
  state: FeaModeState;
  setEnabled(b: boolean): void;
  setChannel(c: string): void;
  setPalette(p: Palette): void;
  /** Returns false (no-op) if min or max is not finite. */
  setRange(r: Range): boolean;
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
  const [state, setState] = createStore<FeaModeStoreState>({
    enabled: false,
    channel: 'vonMises',
    palette: 'viridis',
    range: { ...DEFAULT_RANGE },
    autoEnabledOnce: false,
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

  return { state, setEnabled, setChannel, setPalette, setRange };
}
