/**
 * Buckling-mode animation store — task ι/3458.
 *
 * Holds undeformed base positions, per-mode peak displaced positions, and
 * animation controls (phase, scale, playing, showUndeformed).
 *
 * Animation formula: pos(phase, scale) = base + phase·scale·(peak − base)
 * Phase ramp: ping-pong in [−1, +1] at PHASE_RATE rad/ms when playing.
 */

import { createStore } from 'solid-js/store';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onModeShapeFrame } from '../bridge';
import type { ModeShapeFrame } from '../types';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Phase advances this many units per millisecond when playing. One full sweep
 *  (0 → +1) takes 1 second; a full ping-pong cycle (0 → +1 → −1 → 0) takes 4 s. */
const PHASE_RATE = 1 / 1000;

/** Default scale — the backend already normalises peak displacement to ~10 % of
 *  the bbox diagonal (PRD §8); a frontend scale of 1.0 shows that as-is. */
const DEFAULT_SCALE = 1.0;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/** Per-mode storage: the displaced positions at phase=1 (backend-normalised). */
export interface BucklingModeData {
  peak: number[];
  /** Buckling load multiplier λ, or null when the frame carried no eigenvalue. */
  eigenvalue: number | null;
}

/** Reactive state shape exposed by createBucklingStore(). */
export interface BucklingStoreState {
  /** Undeformed node positions (flat XYZ), null until the first phase≈0 frame. */
  base: number[] | null;
  /** Map from string mode_index → { peak }. */
  modes: Record<string, BucklingModeData>;
  /** Currently-selected mode index, or null. */
  selectedMode: number | null;
  /** Whether the animation is playing. */
  playing: boolean;
  /** Current phase, ∈ [−1, +1]. */
  phase: number;
  /** Current direction of the ping-pong ramp (+1 or −1). */
  direction: number;
  /** Amplitude scale factor (≥ 0, finite). Frontend multiplier on the backend-normalised displacement. */
  scale: number;
  /** Whether to show the undeformed overlay. */
  showUndeformed: boolean;
}

/** Return type of createBucklingStore(). */
export interface BucklingStore {
  state: BucklingStoreState;
  /** Ingest a mode-shape-frame IPC event. */
  ingestFrame(frame: ModeShapeFrame): void;
  /** Sorted ascending list of registered mode indices. */
  modes(): number[];
  /** Select a mode by index for animation. */
  selectMode(i: number): void;
  setPlaying(b: boolean): void;
  togglePlay(): void;
  /** Set scale; rejects non-finite and negative values (no-op). */
  setScale(n: number): void;
  setShowUndeformed(b: boolean): void;
  /** Advance animation by dtMs milliseconds. No-op when playing=false. */
  tick(dtMs: number): void;
  /** Compute current displaced positions using the animation formula. Returns null when base is not ready. */
  currentDisplacedPositions(): number[] | null;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create the buckling animation store.
 *
 * Mirrors the createFeaModeStore factory pattern (SolidJS createStore +
 * finite/non-negative-rejecting setters).
 */
export function createBucklingStore(): BucklingStore {
  const [state, setState] = createStore<BucklingStoreState>({
    base: null,
    modes: {},
    selectedMode: null,
    playing: false,
    phase: 0,
    direction: 1,
    scale: DEFAULT_SCALE,
    showUndeformed: false,
  });

  function ingestFrame(frame: ModeShapeFrame): void {
    if (frame.phase < 0.5) {
      // phase ≈ 0 → undeformed base frame
      setState('base', [...frame.displaced_positions]);
    } else {
      // phase ≈ 1 → peak displaced frame for this mode
      const key = String(frame.mode_index);
      setState('modes', key, { peak: [...frame.displaced_positions], eigenvalue: frame.eigenvalue ?? null });
    }
  }

  function modes(): number[] {
    return Object.keys(state.modes)
      .map(Number)
      .sort((a, b) => a - b);
  }

  function selectMode(i: number): void {
    setState('selectedMode', i);
  }

  function setPlaying(b: boolean): void {
    setState('playing', b);
  }

  function togglePlay(): void {
    setState('playing', !state.playing);
  }

  function setScale(n: number): void {
    if (!Number.isFinite(n) || n < 0) return;
    setState('scale', n);
  }

  function setShowUndeformed(b: boolean): void {
    setState('showUndeformed', b);
  }

  function tick(dtMs: number): void {
    if (!state.playing) return;
    let newPhase = state.phase + state.direction * PHASE_RATE * dtMs;
    let newDirection = state.direction;
    if (newPhase >= 1) {
      newPhase = 1;
      newDirection = -1;
    } else if (newPhase <= -1) {
      newPhase = -1;
      newDirection = 1;
    }
    setState('phase', newPhase);
    setState('direction', newDirection);
  }

  function currentDisplacedPositions(): number[] | null {
    const base = state.base;
    if (!base) return null;
    if (state.selectedMode === null) return base;
    const modeData = state.modes[String(state.selectedMode)];
    if (!modeData) return base;
    const { phase, scale } = state;
    const peak = modeData.peak;
    return base.map((b, i) => b + phase * scale * (peak[i]! - b));
  }

  return {
    state,
    ingestFrame,
    modes,
    selectMode,
    setPlaying,
    togglePlay,
    setScale,
    setShowUndeformed,
    tick,
    currentDisplacedPositions,
  };
}

// ---------------------------------------------------------------------------
// Subscription helper
// ---------------------------------------------------------------------------

/**
 * Subscribe to `mode-shape-frame` IPC events and route them to `store.ingestFrame`.
 * Returns the UnlistenFn so the caller can detach when the component unmounts.
 *
 * Mirrors the pattern used for FEA stores (e.g. subscribeFeaMode) — wraps
 * `onModeShapeFrame` from the bridge.
 */
export async function subscribeModeShapeFrames(store: BucklingStore): Promise<UnlistenFn> {
  return onModeShapeFrame((frame) => store.ingestFrame(frame));
}
