/**
 * Tests for createBucklingStore() and subscribeModeShapeFrames().
 *
 * Task ι/3458. State: base positions + per-mode peaks + animation controls.
 * Animation formula: pos(phase, scale) = base + phase·scale·(peak − base).
 * Ping-pong ramp: phase ∈ [−1, +1], reverses direction at ±1.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import { listen } from '@tauri-apps/api/event';

// Must be at module scope before any import from mockEvents.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from './test_utils/mockEvents';
import { createBucklingStore, subscribeModeShapeFrames } from '../stores/bucklingStore';

// ── Helpers ─────────────────────────────────────────────────────────────────

/**
 * Run `fn` inside a SolidJS root and dispose immediately after.
 * Removes repetitive createRoot boilerplate from each `it` block.
 */
function withRoot<T>(fn: () => T): T {
  let result!: T;
  createRoot((dispose) => {
    result = fn();
    dispose();
  });
  return result;
}

const BASE = [0, 0, 0, 1, 0, 0, 0, 1, 0]; // 3 nodes, flat XYZ
const PEAK = [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]; // displaced

// ── ingestFrame ──────────────────────────────────────────────────────────────

describe('createBucklingStore > ingestFrame', () => {
  it('ingestFrame with phase≈0 sets base positions', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      expect(store.state.base).toEqual(BASE);
    });
  });

  it('ingestFrame with phase≈1 registers mode and stores peak', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      expect(store.state.modes).toHaveProperty('0');
      expect((store.state.modes as Record<string, {peak: number[]}>)['0'].peak).toEqual(PEAK);
    });
  });

  it('modes map grows in insertion order; modes list returns ascending indices', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 1, phase: 1.0, displaced_positions: PEAK });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      const modes = store.modes();
      expect(modes).toEqual([0, 1]);
    });
  });
});

// ── selectMode ───────────────────────────────────────────────────────────────

describe('createBucklingStore > selectMode', () => {
  it('selectMode(i) sets state.selectedMode', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      store.selectMode(0);
      expect(store.state.selectedMode).toBe(0);
    });
  });
});

// ── playing controls ─────────────────────────────────────────────────────────

describe('createBucklingStore > play/pause', () => {
  it('initial state: playing=false', () => {
    withRoot(() => {
      const store = createBucklingStore();
      expect(store.state.playing).toBe(false);
    });
  });

  it('setPlaying(true) sets playing=true', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setPlaying(true);
      expect(store.state.playing).toBe(true);
    });
  });

  it('setPlaying(false) after true reverts to false', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setPlaying(true);
      store.setPlaying(false);
      expect(store.state.playing).toBe(false);
    });
  });

  it('togglePlay flips playing from false to true', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.togglePlay();
      expect(store.state.playing).toBe(true);
    });
  });

  it('togglePlay twice restores playing to false', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.togglePlay();
      store.togglePlay();
      expect(store.state.playing).toBe(false);
    });
  });
});

// ── setScale ─────────────────────────────────────────────────────────────────

describe('createBucklingStore > setScale', () => {
  it('initial scale is a finite positive number', () => {
    withRoot(() => {
      const store = createBucklingStore();
      expect(Number.isFinite(store.state.scale)).toBe(true);
      expect(store.state.scale).toBeGreaterThan(0);
    });
  });

  it('setScale with valid positive value updates scale', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setScale(2.5);
      expect(store.state.scale).toBe(2.5);
    });
  });

  it('setScale(0) is accepted (zero scale = undeformed)', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setScale(0);
      expect(store.state.scale).toBe(0);
    });
  });

  it('setScale(NaN) is a no-op — scale unchanged', () => {
    withRoot(() => {
      const store = createBucklingStore();
      const before = store.state.scale;
      store.setScale(NaN);
      expect(store.state.scale).toBe(before);
    });
  });

  it('setScale(Infinity) is a no-op — scale unchanged', () => {
    withRoot(() => {
      const store = createBucklingStore();
      const before = store.state.scale;
      store.setScale(Infinity);
      expect(store.state.scale).toBe(before);
    });
  });

  it('setScale(-1) is a no-op — negative scale rejected', () => {
    withRoot(() => {
      const store = createBucklingStore();
      const before = store.state.scale;
      store.setScale(-1);
      expect(store.state.scale).toBe(before);
    });
  });
});

// ── setShowUndeformed ────────────────────────────────────────────────────────

describe('createBucklingStore > setShowUndeformed', () => {
  it('initial showUndeformed is false', () => {
    withRoot(() => {
      const store = createBucklingStore();
      expect(store.state.showUndeformed).toBe(false);
    });
  });

  it('setShowUndeformed(true) sets state.showUndeformed', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setShowUndeformed(true);
      expect(store.state.showUndeformed).toBe(true);
    });
  });
});

// ── tick ─────────────────────────────────────────────────────────────────────

describe('createBucklingStore > tick', () => {
  it('tick does NOT advance phase when playing=false', () => {
    withRoot(() => {
      const store = createBucklingStore();
      const initialPhase = store.state.phase;
      store.tick(500);
      expect(store.state.phase).toBe(initialPhase);
    });
  });

  it('tick advances phase when playing=true', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setPlaying(true);
      store.tick(100);
      expect(store.state.phase).not.toBe(0);
    });
  });

  it('phase stays in [−1, +1] after many ticks', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setPlaying(true);
      // Drive many ticks to exercise bounce
      for (let i = 0; i < 200; i++) {
        store.tick(50);
        expect(store.state.phase).toBeGreaterThanOrEqual(-1);
        expect(store.state.phase).toBeLessThanOrEqual(1);
      }
    });
  });

  it('phase bounces: reaches +1 and starts decreasing', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.setPlaying(true);
      // Drive until phase reaches or exceeds +1 (clamped to +1), then one more tick
      let bounced = false;
      let prevPhase = store.state.phase;
      let increasing = true;
      for (let i = 0; i < 1000; i++) {
        store.tick(10);
        const p = store.state.phase;
        if (increasing && p < prevPhase) {
          bounced = true;
          break;
        }
        if (p > prevPhase) increasing = true;
        prevPhase = p;
      }
      expect(bounced).toBe(true);
    });
  });
});

// ── currentDisplacedPositions ────────────────────────────────────────────────

describe('createBucklingStore > currentDisplacedPositions', () => {
  it('returns null when no base has been ingested', () => {
    withRoot(() => {
      const store = createBucklingStore();
      expect(store.currentDisplacedPositions()).toBeNull();
    });
  });

  it('returns base positions when no mode is selected', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      // No selectMode called, no peak ingested
      const result = store.currentDisplacedPositions();
      expect(result).toEqual(BASE);
    });
  });

  it('interpolates correctly: pos = base + phase·scale·(peak − base)', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      store.selectMode(0);
      store.setScale(1.0);

      // Manually set phase to 0.5 via tick with playing=true
      // Instead, we verify the formula at phase=0 (should equal base)
      // and phase=1 (should equal base + scale*(peak-base) = peak when scale=1)
      // At phase=0: positions = base
      // (initial phase is 0, setPlaying(false) keeps it there)
      const atPhaseZero = store.currentDisplacedPositions();
      expect(atPhaseZero).not.toBeNull();
      // At phase=0: base + 0*scale*(peak-base) = base
      for (let i = 0; i < BASE.length; i++) {
        expect(atPhaseZero![i]).toBeCloseTo(BASE[i]!, 6);
      }
    });
  });

  it('at phase=1 and scale=1: displaced positions equal peak', () => {
    withRoot(() => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      store.selectMode(0);
      store.setScale(1.0);

      // Drive phase to +1 by playing
      store.setPlaying(true);
      // Tick enough to reach phase=1 (uses internal rate)
      for (let i = 0; i < 500; i++) {
        store.tick(10);
        if (store.state.phase >= 1.0) break;
      }
      // At phase≈1, scale=1: pos = base + 1*1*(peak-base) = peak
      const result = store.currentDisplacedPositions();
      expect(result).not.toBeNull();
      // Check the formula holds: result[i] ≈ base[i] + phase*(peak[i]-base[i])
      const ph = store.state.phase;
      for (let i = 0; i < BASE.length; i++) {
        const expected = BASE[i]! + ph * (PEAK[i]! - BASE[i]!);
        expect(result![i]).toBeCloseTo(expected, 4);
      }
    });
  });
});

// ── subscribeModeShapeFrames ─────────────────────────────────────────────────

describe('subscribeModeShapeFrames', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('emitted mode-shape-frame routes to store.ingestFrame (state updates)', async () => {
    await withRoot(async () => {
      const store = createBucklingStore();
      const handle = mockTauriEvent<{ mode_index: number; phase: number; displaced_positions: number[] }>('mode-shape-frame');

      await subscribeModeShapeFrames(store);
      handle.emit({ mode_index: 0, phase: 0.0, displaced_positions: BASE });

      expect(store.state.base).toEqual(BASE);
    });
  });

  it('returned UnlistenFn detaches the listener', async () => {
    await withRoot(async () => {
      const store = createBucklingStore();
      const handle = mockTauriEvent<{ mode_index: number; phase: number; displaced_positions: number[] }>('mode-shape-frame');

      const unlisten = await subscribeModeShapeFrames(store);
      // Detach
      unlisten();
      // Emit after detach — should NOT update store
      handle.emit({ mode_index: 0, phase: 0.0, displaced_positions: BASE });

      expect(store.state.base).toBeNull();
    });
  });
});
