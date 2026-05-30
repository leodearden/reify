/**
 * BucklingPanel component tests. Task ι/3458.
 *
 * Props-driven panel with a `store: BucklingStore` prop.
 * Mirrors SolverProgressOverlay.test.tsx / FeaCasePickerDropdown.test.tsx
 * in structure. Must render under jsdom without a WebGL context.
 *
 * Covers:
 * (a) empty state: placeholder text, no mode rows
 * (b) renders one selectable row per registered mode
 * (c) clicking a mode row calls store.selectMode
 * (d) clicking play/pause toggles store.playing
 * (e) moving the scale slider calls store.setScale
 * (f) toggling the undeformed-overlay checkbox calls store.setShowUndeformed
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';

// Mock three.js — the panel imports bucklingAnimator which imports from 'three'.
// jsdom has no WebGL context, so guard means the 3D path never executes, but
// vi.mock prevents the module-resolution error.
vi.mock('three', () => ({
  BufferGeometry: class {
    attributes: Record<string, unknown> = {};
    setAttribute = vi.fn();
    getAttribute = vi.fn(() => ({ array: new Float32Array(9), needsUpdate: false }));
    dispose = vi.fn();
  },
  Float32BufferAttribute: class {
    array: Float32Array;
    itemSize: number;
    needsUpdate = false;
    constructor(arr: Float32Array, itemSize: number) {
      this.array = arr;
      this.itemSize = itemSize;
    }
  },
  Points: class {
    visible = true;
    constructor(_g: unknown, _m: unknown) {}
  },
  PointsMaterial: class {
    dispose = vi.fn();
    constructor(_opts?: unknown) {}
  },
}));

import { createBucklingStore } from '../stores/bucklingStore';
import { BucklingPanel } from '../panels/BucklingPanel';

// ── Helpers ─────────────────────────────────────────────────────────────────

const BASE = [0, 0, 0, 1, 0, 0, 0, 1, 0];
const PEAK = [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0];

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('BucklingPanel', () => {
  it('(a) renders a placeholder / empty state when no modes are registered', () => {
    let store: ReturnType<typeof createBucklingStore>;
    createRoot((dispose) => {
      store = createBucklingStore();
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    // Should render the panel container
    expect(screen.getByTestId('buckling-panel')).toBeTruthy();
    // No mode rows — no listitem or row elements with "Mode" label
    expect(screen.queryByText(/Mode \d/)).toBeNull();
  });

  it('(b) renders one selectable row per registered mode (label "Mode N")', () => {
    let store: ReturnType<typeof createBucklingStore>;
    createRoot((dispose) => {
      store = createBucklingStore();
      // Register two modes
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      store.ingestFrame({ mode_index: 1, phase: 1.0, displaced_positions: PEAK });
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    // Expect "Mode 1" and "Mode 2" labels (1-indexed for display)
    expect(screen.getByText(/Mode 1/)).toBeTruthy();
    expect(screen.getByText(/Mode 2/)).toBeTruthy();
  });

  it('(c) clicking a mode row calls store.selectMode with the mode index', () => {
    let store: ReturnType<typeof createBucklingStore>;
    const selectModeSpy = vi.fn();
    createRoot((dispose) => {
      store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      store.ingestFrame({ mode_index: 0, phase: 1.0, displaced_positions: PEAK });
      // Spy on selectMode
      const origSelectMode = store!.selectMode;
      store!.selectMode = (i: number) => { selectModeSpy(i); origSelectMode(i); };
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    const modeRow = screen.getByText(/Mode 1/);
    fireEvent.click(modeRow);
    expect(selectModeSpy).toHaveBeenCalledWith(0);
  });

  it('(d) clicking the play/pause button calls store.togglePlay', () => {
    let store: ReturnType<typeof createBucklingStore>;
    const togglePlaySpy = vi.fn();
    createRoot((dispose) => {
      store = createBucklingStore();
      const orig = store!.togglePlay;
      store!.togglePlay = () => { togglePlaySpy(); orig(); };
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    const playBtn = screen.getByTestId('buckling-play-pause');
    fireEvent.click(playBtn);
    expect(togglePlaySpy).toHaveBeenCalledOnce();
  });

  it('(e) changing the scale slider calls store.setScale with the parsed value', () => {
    let store: ReturnType<typeof createBucklingStore>;
    const setScaleSpy = vi.fn();
    createRoot((dispose) => {
      store = createBucklingStore();
      const orig = store!.setScale;
      store!.setScale = (n: number) => { setScaleSpy(n); orig(n); };
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    const slider = screen.getByTestId('buckling-scale-slider');
    fireEvent.input(slider, { target: { value: '2.5' } });
    expect(setScaleSpy).toHaveBeenCalledWith(2.5);
  });

  it('(f) toggling the undeformed-overlay checkbox calls store.setShowUndeformed', () => {
    let store: ReturnType<typeof createBucklingStore>;
    const setShowSpy = vi.fn();
    createRoot((dispose) => {
      store = createBucklingStore();
      const orig = store!.setShowUndeformed;
      store!.setShowUndeformed = (b: boolean) => { setShowSpy(b); orig(b); };
      render(() => <BucklingPanel store={store!} />);
      dispose();
    });
    const checkbox = screen.getByTestId('buckling-show-undeformed');
    fireEvent.click(checkbox);
    expect(setShowSpy).toHaveBeenCalledWith(true);
  });
});
