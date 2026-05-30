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
 * (g) render-path: constructs scene/renderer and renders each frame
 * (h) render-path: disposes the renderer on cleanup
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';

// ── Spy holders — module-level so vi.mock factory can close over them.
//    The factory is evaluated lazily (on first import of 'three'), so these
//    will be initialized by then.  Pattern matches Viewport.test.tsx.
let sceneAddSpy = vi.fn();
let renderSpy = vi.fn();
let rendererCtorSpy = vi.fn();
let rendererDisposeSpy = vi.fn();

// Mock three.js — the panel imports bucklingAnimator which imports from 'three'.
// jsdom has no WebGL context, so guard means the 3D path never executes for
// the (a)–(f) tests, but vi.mock prevents the module-resolution error.
// Scene/PerspectiveCamera/WebGLRenderer are added here so the render-path
// tests can exercise the full init branch.
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
  // Scene: delegates add to the module-level spy via method (not class field)
  // so the closure captures the binding, not the value at class-definition time.
  Scene: class {
    add(...args: unknown[]) { return sceneAddSpy(...args); }
    remove = vi.fn();
  },
  PerspectiveCamera: class {
    position = { set: vi.fn() };
    up = { set: vi.fn() };
    lookAt = vi.fn();
    updateProjectionMatrix = vi.fn();
    constructor(_fov?: number, _aspect?: number, _near?: number, _far?: number) {}
  },
  WebGLRenderer: class {
    setSize = vi.fn();
    render(...args: unknown[]) { return renderSpy(...args); }
    dispose(...args: unknown[]) { return rendererDisposeSpy(...args); }
    constructor(opts?: unknown) {
      rendererCtorSpy(opts);
    }
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
  // Reinitialize spy holders so clearAllMocks doesn't break the delegation
  sceneAddSpy = vi.fn();
  renderSpy = vi.fn();
  rendererCtorSpy = vi.fn();
  rendererDisposeSpy = vi.fn();
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

  // ── Render-path tests (getContext→truthy, rAF captured) ────────────────────
  //
  // These stubs are scoped to this describe so the (a)–(f) tests above still
  // hit getContext→null (the jsdom default) and return at the WebGL guard.

  describe('render path', () => {
    let capturedFrame: ((time: number) => void) | null = null;

    beforeEach(() => {
      capturedFrame = null;
      // Make getContext('webgl') return a truthy value so onMount's guard passes.
      vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({} as any);
      // Capture only the FIRST rAF callback; subsequent reschedules are ignored
      // so frame() doesn't recurse infinitely.
      vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        if (capturedFrame === null) capturedFrame = cb as (time: number) => void;
        return 1;
      });
      vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {});
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it('(g) adds object3d and undeformedOverlay to scene, binds renderer to canvas, renders each frame', () => {
      let store: ReturnType<typeof createBucklingStore>;
      createRoot((dispose) => {
        store = createBucklingStore();
        // Seed base so the WebGL branch runs
        store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
        render(() => <BucklingPanel store={store!} />);
        dispose();
      });

      // scene.add should have been called with the two Points objects
      expect(sceneAddSpy).toHaveBeenCalledTimes(2);

      // renderer constructor should have received opts.canvas = the panel canvas
      expect(rendererCtorSpy).toHaveBeenCalledTimes(1);
      const rendererOpts = rendererCtorSpy.mock.calls[0]![0] as { canvas: HTMLCanvasElement };
      expect(rendererOpts.canvas).toBeInstanceOf(HTMLCanvasElement);

      // Invoke the captured frame callback and assert render was called
      expect(capturedFrame).not.toBeNull();
      capturedFrame!(100);
      expect(renderSpy).toHaveBeenCalledTimes(1);
    });

    it('(h) disposes the renderer on root cleanup', () => {
      const store = createBucklingStore();
      store.ingestFrame({ mode_index: 0, phase: 0.0, displaced_positions: BASE });
      const { unmount } = render(() => <BucklingPanel store={store} />);

      // Invoke one frame so the renderer is active
      capturedFrame?.(100);

      // unmount() disposes the render root, triggering Solid's onCleanup
      unmount();
      expect(rendererDisposeSpy).toHaveBeenCalledTimes(1);
    });
  });
});
