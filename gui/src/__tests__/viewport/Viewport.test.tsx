import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import type { MeshData, VisibilityState, TensegritySurfaceData } from '../../types';
import { createFeaModeStore } from '../../stores';

// Stub ResizeObserver for jsdom (which doesn't support it)
globalThis.ResizeObserver = class ResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: ResizeObserverCallback) {}
};

// RAF callback capture mechanism for race condition testing
let rafCallbacks: Array<FrameRequestCallback> = [];
let rafIdCounter = 1;

globalThis.requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => {
  rafCallbacks.push(cb);
  return rafIdCounter++;
}) as unknown as typeof requestAnimationFrame;
globalThis.cancelAnimationFrame = vi.fn((_id: number) => {}) as unknown as typeof cancelAnimationFrame;

// Mock three.js Box3 used for scene bounds computation
vi.mock('three', () => ({
  Box3: vi.fn(() => ({
    expandByObject: vi.fn(),
    isEmpty: vi.fn(() => true),
    getCenter: vi.fn(),
    getSize: vi.fn(),
  })),
}));

// Hoisted mock for bakeColours (must be hoisted so vi.mock factory can reference it)
const mockBakeColours = vi.hoisted(() =>
  vi.fn((_scalars: Float32Array, _range: unknown, _palette: string) => new Float32Array(6))
);

// Mock the colormap module so Viewport's bake closure is testable
vi.mock('../../viewport/colormap', () => ({
  bakeColours: mockBakeColours,
}));

// Mock the viewport modules
const mockResize = vi.fn();
const mockRendererRender = vi.fn();
const mockRendererDispose = vi.fn();
const mockRendererSetSize = vi.fn();

const mockControlsUpdate = vi.fn();
const mockControlsDispose = vi.fn();

const mockMeshSync = vi.fn();
const mockMeshDispose = vi.fn();
const mockMeshGetSceneMeshes = vi.fn(() => new Map());
const mockMeshGetGhostMeshes = vi.fn(() => new Map());
const mockMeshSetVisibility = vi.fn();
const mockMeshSetColorize = vi.fn();
const mockMeshRebuildMaterials = vi.fn();
const mockMeshSetDeformation = vi.fn();
const mockMeshGetDeformedOverlays = vi.fn(() => new Map());

const mockGrid = { type: 'GridHelper', visible: true };
const mockAxes = { type: 'AxesHelper', visible: true };
const mockAxisLabels = { type: 'Group', visible: true };
const mockDisposeAxisLabels = vi.fn();

// Camera stub with position/up set-spies and mutable zoom — shared across tests
const mockCameraPositionSet = vi.fn();
const mockCameraUpSet = vi.fn();
const mockCameraUpdateProjectionMatrix = vi.fn();
const mockCamera = {
  type: 'PerspectiveCamera',
  position: { set: mockCameraPositionSet, x: 0, y: 0, z: 0 },
  up: { set: mockCameraUpSet, x: 0, y: 0, z: 0 },
  zoom: 1,
  updateProjectionMatrix: mockCameraUpdateProjectionMatrix,
};

vi.mock('../../viewport/scene', () => ({
  createScene: vi.fn(() => ({
    scene: { type: 'Scene' },
    camera: mockCamera,
    renderer: {
      render: mockRendererRender,
      dispose: mockRendererDispose,
      setSize: mockRendererSetSize,
      domElement: document.createElement('canvas'),
    },
    resize: mockResize,
    adjustClipping: vi.fn(),
    grid: mockGrid,
    axes: mockAxes,
    axisLabels: mockAxisLabels,
    disposeAxisLabels: mockDisposeAxisLabels,
  })),
}));

// Captured event listeners from the controls mock (for render-on-demand tests)
let controlsListeners: Record<string, Function[]> = {};

// Controls stub with target.set spy — shared across tests
const mockControlsTargetSet = vi.fn();
const mockControlsTarget = { set: mockControlsTargetSet, x: 0, y: 0, z: 0 };

vi.mock('../../viewport/controls', () => ({
  createControls: vi.fn(() => ({
    controls: {
      addEventListener: vi.fn((event: string, cb: Function) => {
        if (!controlsListeners[event]) controlsListeners[event] = [];
        controlsListeners[event].push(cb);
      }),
      removeEventListener: vi.fn((_event: string, _cb: Function) => {}),
      target: mockControlsTarget,
    },
    update: mockControlsUpdate,
    dispose: mockControlsDispose,
  })),
}));

vi.mock('../../viewport/meshManager', () => ({
  createMeshManager: vi.fn(() => ({
    sync: mockMeshSync,
    dispose: mockMeshDispose,
    getSceneMeshes: mockMeshGetSceneMeshes,
    getGhostMeshes: mockMeshGetGhostMeshes,
    setVisibility: mockMeshSetVisibility,
    setColorize: mockMeshSetColorize,
    rebuildMaterials: mockMeshRebuildMaterials,
    setDeformation: mockMeshSetDeformation,
    getDeformedOverlays: mockMeshGetDeformedOverlays,
  })),
}));

// ── wireManager mock (T0b) ────────────────────────────────────────────────────
const mockWireSync = vi.fn();
const mockWireDispose = vi.fn();
const mockWireSetResolution = vi.fn();

vi.mock('../../viewport/wireManager', () => ({
  createWireManager: vi.fn(() => ({
    sync: mockWireSync,
    dispose: mockWireDispose,
    setResolution: mockWireSetResolution,
  })),
}));

// ── surfaceManager mock (β) ───────────────────────────────────────────────────
const mockSurfaceSync = vi.fn();
const mockSurfaceDispose = vi.fn();

vi.mock('../../viewport/surfaceManager', () => ({
  createSurfaceManager: vi.fn(() => ({
    sync: mockSurfaceSync,
    dispose: mockSurfaceDispose,
  })),
}));

const mockSelectionSetHovered = vi.fn();
const mockSelectionSetSelected = vi.fn();
const mockSelectionRefreshSelected = vi.fn();
const mockSelectionFitToView = vi.fn();
const mockSelectionFlyToEntity = vi.fn();
const mockSelectionInvalidateRect = vi.fn();
const mockSelectionDispose = vi.fn();

vi.mock('../../viewport/selection', () => ({
  createSelection: vi.fn(() => ({
    setHovered: mockSelectionSetHovered,
    setSelected: mockSelectionSetSelected,
    refreshSelected: mockSelectionRefreshSelected,
    fitToView: mockSelectionFitToView,
    flyToEntity: mockSelectionFlyToEntity,
    invalidateRect: mockSelectionInvalidateRect,
    dispose: mockSelectionDispose,
  })),
}));

import { Viewport } from '../../viewport';
import { createSelection } from '../../viewport/selection';

beforeEach(() => {
  vi.clearAllMocks();
  rafCallbacks = [];
  rafIdCounter = 1;
  controlsListeners = {};
  mockGrid.visible = true;
  mockAxes.visible = true;
  mockAxisLabels.visible = true;
  // Reset camera mutable state
  mockCamera.zoom = 1;
  mockControlsTarget.x = 0;
  mockControlsTarget.y = 0;
  mockControlsTarget.z = 0;
  // Reset FEA mock state
  mockMeshSetColorize.mockClear();
  mockMeshRebuildMaterials.mockClear();
  mockMeshSetDeformation.mockClear();
  mockMeshGetDeformedOverlays.mockClear();
  mockBakeColours.mockClear();
});

// Module-scope FEA mesh factory — shared by 'Viewport FEA auto-range' and
// 'Viewport FEA Lock Current + readout wiring' suites to avoid drift.
function makeFEAMesh(values: number[]): MeshData {
  return {
    entity_path: 'bracket',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
    scalar_channels: { vonMises: new Float32Array(values) },
  };
}

describe('Viewport', () => {
  it('renders a canvas element with data-testid viewport-canvas', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    expect(screen.getByTestId('viewport-canvas')).toBeTruthy();
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.tagName.toLowerCase()).toBe('canvas');
  });

  it('canvas is wrapped in a container div with data-testid viewport-container', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const container = screen.getByTestId('viewport-container');
    expect(container).toBeTruthy();
    expect(container.tagName.toLowerCase()).toBe('div');
    // Canvas should be inside the container
    const canvas = screen.getByTestId('viewport-canvas');
    expect(container.contains(canvas)).toBe(true);
  });

  it('shows tooltip with entity name when hoveredEntity is set', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" hoveredEntity="bracket/hole" />);
    const tooltip = screen.getByTestId('viewport-tooltip');
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain('bracket/hole');
  });

  it('hides tooltip when hoveredEntity is null', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" hoveredEntity={null} />);
    expect(screen.queryByTestId('viewport-tooltip')).toBeNull();
  });

  it('shows spinner overlay when evalStatus phase is evaluating', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" evalStatus={{ phase: 'evaluating' }} />);
    const spinner = screen.getByTestId('viewport-spinner');
    expect(spinner).toBeTruthy();
  });

  it('hides spinner when evalStatus phase is idle', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" evalStatus={{ phase: 'idle' }} />);
    expect(screen.queryByTestId('viewport-spinner')).toBeNull();
  });

  it('renders fit-to-view button with data-testid', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const btn = screen.getByTestId('fit-to-view');
    expect(btn).toBeTruthy();
  });

  it('hides spinner when evalStatus is not provided', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    expect(screen.queryByTestId('viewport-spinner')).toBeNull();
  });

  it('clicking fit-to-view button calls selection.fitToView', async () => {
    const onFitToView = vi.fn();
    render(() => <Viewport meshes={{}} viewportId="test-vp" onFitToView={onFitToView} />);
    const btn = screen.getByTestId('fit-to-view');
    fireEvent.click(btn);

    // selection.fitToView should have been called (bridged via mutable ref)
    expect(mockSelectionFitToView).toHaveBeenCalled();
    // The onFitToView prop callback should also be called
    expect(onFitToView).toHaveBeenCalled();
  });

  it('selectedEntity effect re-runs setSelected when props.meshes changes', () => {
    const initialMeshes: Record<string, MeshData> = {
      'bracket/plate': { entity_path: 'bracket/plate', vertices: new Float32Array([0, 0, 0]), indices: new Uint32Array([0]), normals: null },
    };
    const updatedMeshes: Record<string, MeshData> = {
      'bracket/plate': { entity_path: 'bracket/plate', vertices: new Float32Array([1, 1, 1]), indices: new Uint32Array([0]), normals: null },
    };

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>(initialMeshes);

    render(() => <Viewport meshes={meshes()} viewportId="test-vp" selectedEntity="bracket/plate" />);

    // After initial render, setSelected should have been called with the entity
    expect(mockSelectionSetSelected).toHaveBeenCalledWith('bracket/plate');
    const initialCallCount = mockSelectionSetSelected.mock.calls.length;

    // Update meshes (simulating engine re-evaluation)
    setMeshes(updatedMeshes);

    // setSelected should be called again (to rebuild wireframe from updated geometry)
    expect(mockSelectionSetSelected.mock.calls.length).toBeGreaterThan(initialCallCount);
    // The last call should still be with the same entity
    const lastCall = mockSelectionSetSelected.mock.calls[mockSelectionSetSelected.mock.calls.length - 1];
    expect(lastCall[0]).toBe('bracket/plate');
  });

  it('animate loop does not call renderer.render after cleanup/dispose', () => {
    const { unmount } = render(() => <Viewport meshes={{}} viewportId="test-vp" />);

    // The initial animate() call should have been scheduled
    expect(rafCallbacks.length).toBeGreaterThan(0);

    // Clear the render mock to track only post-cleanup calls
    mockRendererRender.mockClear();

    // Unmount triggers onCleanup
    unmount();

    // Now manually invoke any captured RAF callback (simulating a pending frame firing after dispose)
    const cb = rafCallbacks[rafCallbacks.length - 1];
    if (cb) {
      cb(performance.now());
    }

    // renderer.render should NOT have been called after cleanup
    expect(mockRendererRender).not.toHaveBeenCalled();
  });

  it('calls flyToEntityRef callback with a function on mount', () => {
    const flyToEntityRef = vi.fn();
    render(() => <Viewport meshes={{}} viewportId="test-vp" flyToEntityRef={flyToEntityRef} />);
    expect(flyToEntityRef).toHaveBeenCalledTimes(1);
    expect(typeof flyToEntityRef.mock.calls[0][0]).toBe('function');
  });

  it('flyToEntityRef function delegates to selection.flyToEntity', () => {
    let capturedFn: ((entityPath: string) => void) | undefined;
    const flyToEntityRef = vi.fn((fn: (entityPath: string) => void) => {
      capturedFn = fn;
    });
    render(() => <Viewport meshes={{}} viewportId="test-vp" flyToEntityRef={flyToEntityRef} />);

    expect(capturedFn).toBeDefined();
    capturedFn!('Bracket');
    expect(mockSelectionFlyToEntity).toHaveBeenCalledWith('Bracket');
  });

  it('animate loop does NOT call renderer.render when idle after initial frame', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);

    // First rAF callback fires the initial render (animate calls rAF, then renders)
    expect(rafCallbacks.length).toBeGreaterThan(0);
    const firstCb = rafCallbacks[0];
    firstCb(performance.now()); // initial frame — should render

    expect(mockRendererRender).toHaveBeenCalled();
    mockRendererRender.mockClear();

    // Second rAF callback (scheduled by the first animate call)
    // With render-on-demand, this should NOT render since nothing changed
    const secondCb = rafCallbacks[rafCallbacks.length - 1];
    secondCb(performance.now());

    expect(mockRendererRender).not.toHaveBeenCalled();
  });

  it('controls change event triggers re-render on next frame', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);

    // Fire first rAF callback (initial render)
    const firstCb = rafCallbacks[0];
    firstCb(performance.now());
    mockRendererRender.mockClear();

    // Simulate controls 'change' event (camera moved)
    expect(controlsListeners['change']).toBeDefined();
    controlsListeners['change'][0]();

    // Fire next rAF — should render since change event set dirty flag
    const nextCb = rafCallbacks[rafCallbacks.length - 1];
    nextCb(performance.now());

    expect(mockRendererRender).toHaveBeenCalledTimes(1);
  });

  it('resize triggers re-render on next frame', () => {
    // Capture the ResizeObserver callback
    let roCallback: ResizeObserverCallback | undefined;
    const OrigRO = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
      constructor(cb: ResizeObserverCallback) { roCallback = cb; }
    } as any;

    try {
      render(() => <Viewport meshes={{}} viewportId="test-vp" />);

      // Fire first rAF callback (initial render)
      const firstCb = rafCallbacks[0];
      firstCb(performance.now());
      mockRendererRender.mockClear();

      // Simulate resize
      roCallback!([{ contentRect: { width: 1024, height: 768 } }] as any, {} as any);

      // Fire next rAF — should render since resize sets dirty flag
      const nextCb = rafCallbacks[rafCallbacks.length - 1];
      nextCb(performance.now());

      expect(mockRendererRender).toHaveBeenCalledTimes(1);
    } finally {
      // Restore original ResizeObserver regardless of assertion failures
      globalThis.ResizeObserver = OrigRO;
    }
  });

  it('passes controls to createSelection for orbit target updates', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);

    // createSelection should have been called with controls from createControls
    const mockCreateSelection = createSelection as unknown as ReturnType<typeof vi.fn>;
    expect(mockCreateSelection).toHaveBeenCalledTimes(1);
    const opts = mockCreateSelection.mock.calls[0][0];
    expect(opts).toHaveProperty('controls');
    // The controls value should be the OrbitControls instance from createControls mock
    // createControls mock returns { controls: { addEventListener, removeEventListener }, ... }
    expect(opts.controls).toBeDefined();
    expect(typeof opts.controls.addEventListener).toBe('function');
  });

  it('calls fitToViewRef callback with a function on mount', () => {
    const fitToViewRef = vi.fn();
    render(() => <Viewport meshes={{}} viewportId="test-vp" fitToViewRef={fitToViewRef} />);
    expect(fitToViewRef).toHaveBeenCalledTimes(1);
    expect(typeof fitToViewRef.mock.calls[0][0]).toBe('function');
  });

  it('fitToViewRef function delegates to selection.fitToView', () => {
    let capturedFn: (() => void) | undefined;
    const fitToViewRef = vi.fn((fn: () => void) => {
      capturedFn = fn;
    });
    render(() => <Viewport meshes={{}} viewportId="test-vp" fitToViewRef={fitToViewRef} />);

    expect(capturedFn).toBeDefined();
    mockSelectionFitToView.mockClear();
    capturedFn!();
    expect(mockSelectionFitToView).toHaveBeenCalled();
  });

  it('renders a grid toggle button with data-testid toggle-grid', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const btn = screen.getByTestId('toggle-grid');
    expect(btn).toBeTruthy();
  });

  it('clicking toggle-grid button toggles grid and axes visible state', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const btn = screen.getByTestId('toggle-grid');

    expect(mockGrid.visible).toBe(true);
    expect(mockAxes.visible).toBe(true);

    fireEvent.click(btn);

    expect(mockGrid.visible).toBe(false);
    expect(mockAxes.visible).toBe(false);

    fireEvent.click(btn);

    expect(mockGrid.visible).toBe(true);
    expect(mockAxes.visible).toBe(true);
  });

  it('clicking toggle-grid button toggles axisLabels visible in lockstep with grid and axes', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const btn = screen.getByTestId('toggle-grid');

    expect(mockAxisLabels.visible).toBe(true);

    fireEvent.click(btn);

    expect(mockAxisLabels.visible).toBe(false);

    fireEvent.click(btn);

    expect(mockAxisLabels.visible).toBe(true);
  });

  it('tooltip style top/left update on mousemove within container', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" hoveredEntity="bracket/plate" />);
    const container = screen.getByTestId('viewport-container');
    const tooltip = screen.getByTestId('viewport-tooltip');

    // Simulate container with known position
    vi.spyOn(container, 'getBoundingClientRect').mockReturnValue({
      top: 100, left: 200, width: 800, height: 600,
      right: 1000, bottom: 700, x: 200, y: 100, toJSON: () => {},
    });

    // Simulate mouse move to (350, 250) in screen coordinates
    // Container is at (200, 100), so relative position is (150, 150)
    fireEvent.mouseMove(container, { clientX: 350, clientY: 250 });

    // Tooltip should be positioned near the pointer (with offset)
    const top = parseInt(tooltip.style.top, 10);
    const left = parseInt(tooltip.style.left, 10);

    // Should be near pointer position, not at fixed 8px/8px
    expect(top).toBeGreaterThan(100);
    expect(left).toBeGreaterThan(100);
  });

  it('tooltip is not positioned at fixed 8px/8px after a mousemove event', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" hoveredEntity="bracket/plate" />);
    const container = screen.getByTestId('viewport-container');
    const tooltip = screen.getByTestId('viewport-tooltip');

    vi.spyOn(container, 'getBoundingClientRect').mockReturnValue({
      top: 0, left: 0, width: 800, height: 600,
      right: 800, bottom: 600, x: 0, y: 0, toJSON: () => {},
    });

    fireEvent.mouseMove(container, { clientX: 400, clientY: 300 });

    // After a mouse move, tooltip should NOT be at the initial fixed position
    expect(tooltip.style.top).not.toBe('8px');
    expect(tooltip.style.left).not.toBe('8px');
  });

  it('tooltip still shows the hoveredEntity text content', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" hoveredEntity="bracket/plate" />);
    const tooltip = screen.getByTestId('viewport-tooltip');
    expect(tooltip.textContent).toContain('bracket/plate');
  });
});

describe('Viewport accessibility', () => {
  it('canvas has tabindex="0" so it can receive keyboard focus', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.getAttribute('tabindex')).toBe('0');
  });

  it('canvas has aria-label="3D viewport"', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.getAttribute('aria-label')).toBe('3D viewport');
  });
});

// Note: these tests assert on mock call counts synchronously after calling the signal setter.
// SolidJS createSignal setters synchronously flush effects in the default owner context used
// by @solidjs/testing-library's render(), so the assertions are reliable. If that ever changes
// (e.g. under batch() or Suspense), wrap assertions in a waitFor() from the testing library.
describe('Viewport entityVisibility reset', () => {
  it('resets removed entities to show when entityVisibility prop drops keys', () => {
    const [entityVisibility, setEntityVisibility] = createSignal<Record<string, VisibilityState> | undefined>(
      { 'bracket/plate': 'ghost', 'bracket/hole': 'hidden' }
    );

    render(() => <Viewport meshes={{}} viewportId="test-vp" entityVisibility={entityVisibility()} />);

    // After initial render, setVisibility should be called for both entities
    expect(mockMeshSetVisibility).toHaveBeenCalledWith('bracket/plate', 'ghost');
    expect(mockMeshSetVisibility).toHaveBeenCalledWith('bracket/hole', 'hidden');

    // Clear mock, then drop 'bracket/hole' from the visibility prop
    mockMeshSetVisibility.mockClear();
    setEntityVisibility({ 'bracket/plate': 'ghost' });

    // The removed 'bracket/hole' should be reset to 'show'
    expect(mockMeshSetVisibility).toHaveBeenCalledWith('bracket/hole', 'show');
  });

  it('resets all entities to show when entityVisibility prop is cleared to undefined', () => {
    const [entityVisibility, setEntityVisibility] = createSignal<Record<string, VisibilityState> | undefined>(
      { 'bracket/plate': 'ghost', 'bracket/hole': 'hidden' }
    );

    render(() => <Viewport meshes={{}} viewportId="test-vp" entityVisibility={entityVisibility()} />);

    // Clear mock, then clear entityVisibility entirely
    mockMeshSetVisibility.mockClear();
    setEntityVisibility(undefined);

    // Both entities should be reset to 'show'
    expect(mockMeshSetVisibility).toHaveBeenCalledWith('bracket/plate', 'show');
    expect(mockMeshSetVisibility).toHaveBeenCalledWith('bracket/hole', 'show');
  });
});

describe('Viewport multi-selection props', () => {
  it('selectedEntities prop routes an array of paths to selection.setSelected', () => {
    // selectedEntities is not yet a known ViewportProps field — test fails until step-28 impl
    render(() => <Viewport meshes={{}} viewportId="test-vp" selectedEntities={['A', 'B'] as any} />);
    // setSelected should be called with the list (not the scalar selectedEntity path)
    expect(mockSelectionSetSelected).toHaveBeenCalledWith(['A', 'B']);
  });

  it('onSelect prop receives (path, modifiers) when the selection module fires with modifiers', () => {
    const onSelectSpy = vi.fn();
    render(() => <Viewport meshes={{}} viewportId="test-vp" onSelect={onSelectSpy as any} />);

    // Retrieve the onSelect callback that Viewport passed to createSelection
    const mockCreateSelection = createSelection as unknown as ReturnType<typeof vi.fn>;
    const selectionOpts = mockCreateSelection.mock.calls[mockCreateSelection.mock.calls.length - 1][0];

    // Simulate the selection module calling onSelect with modifiers
    selectionOpts.onSelect('A', { ctrl: true, shift: false });

    // Viewport must forward both the path AND the modifiers to props.onSelect
    expect(onSelectSpy).toHaveBeenCalledWith('A', { ctrl: true, shift: false });
  });

  it('plain click (no modifiers) forwards { ctrl: false, shift: false } to onSelect prop', () => {
    const onSelectSpy = vi.fn();
    render(() => <Viewport meshes={{}} viewportId="test-vp" onSelect={onSelectSpy as any} />);

    const mockCreateSelection = createSelection as unknown as ReturnType<typeof vi.fn>;
    const selectionOpts = mockCreateSelection.mock.calls[mockCreateSelection.mock.calls.length - 1][0];

    selectionOpts.onSelect('B', { ctrl: false, shift: false });

    expect(onSelectSpy).toHaveBeenCalledWith('B', { ctrl: false, shift: false });
  });
});

describe('Viewport viewportId and camera restore', () => {
  it('renders without crash when viewportId is provided but no viewportStore — camera stubs untouched', () => {
    render(() => <Viewport meshes={{}} viewportId="design-main" />);
    // Camera position/up set-spies must NOT be called when there is no store to restore from
    expect(mockCameraPositionSet).not.toHaveBeenCalled();
    expect(mockCameraUpSet).not.toHaveBeenCalled();
    expect(mockControlsTargetSet).not.toHaveBeenCalled();
    // zoom should remain at its default value
    expect(mockCamera.zoom).toBe(1);
  });

  it('applies saved camera from viewportStore on mount', () => {
    const savedCamera = {
      position: [7, 8, 9] as [number, number, number],
      target: [1, 2, 3] as [number, number, number],
      up: [0, 0, 1] as [number, number, number],
      zoom: 4,
    };
    const fakeStore = {
      state: {} as any,
      getViewport: vi.fn((_id: string) => ({ camera: savedCamera })),
      setActiveViewport: vi.fn(),
      assignView: vi.fn(),
      updateCamera: vi.fn(),
    };

    render(() => <Viewport meshes={{}} viewportId="design-main" viewportStore={fakeStore as any} />);

    // camera.position.set should be called with the saved position
    expect(mockCameraPositionSet).toHaveBeenCalledWith(7, 8, 9);
    // camera.up.set should be called with the saved up vector
    expect(mockCameraUpSet).toHaveBeenCalledWith(0, 0, 1);
    // controls.target.set should be called with the saved target
    expect(mockControlsTargetSet).toHaveBeenCalledWith(1, 2, 3);
    // camera.zoom should be set to the saved zoom value
    expect(mockCamera.zoom).toBe(4);
  });
});

describe('Viewport debug bridge controls exposure', () => {
  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('exposes controls on window.__REIFY_DEBUG__.viewport after mount', () => {
    window.__REIFY_DEBUG__ = { stores: {} as any };
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);

    const controls = window.__REIFY_DEBUG__?.viewport?.controls;
    expect(controls).toBeDefined();
    expect(typeof controls!.addEventListener).toBe('function');
  });
});

describe('Viewport camera persistence', () => {
  it('calls viewportStore.updateCamera with current camera state when controls fire end', () => {
    const mockUpdateCamera = vi.fn();
    const fakeStore = {
      state: {} as any,
      getViewport: vi.fn((_id: string) => undefined),
      setActiveViewport: vi.fn(),
      assignView: vi.fn(),
      updateCamera: mockUpdateCamera,
    };

    render(() => <Viewport meshes={{}} viewportId="design-main" viewportStore={fakeStore as any} />);

    // Set the camera/controls stubs to a known state before firing end
    mockCamera.position.x = 10;
    mockCamera.position.y = 20;
    mockCamera.position.z = 30;
    mockCamera.up.x = 0;
    mockCamera.up.y = 1;
    mockCamera.up.z = 0;
    mockCamera.zoom = 2;
    mockControlsTarget.x = 5;
    mockControlsTarget.y = 6;
    mockControlsTarget.z = 7;

    // Fire the captured 'end' listener synchronously (fires once when interaction ends)
    expect(controlsListeners['end']).toBeDefined();
    expect(controlsListeners['end'].length).toBeGreaterThan(0);
    for (const cb of controlsListeners['end']) {
      cb();
    }

    // viewportStore.updateCamera should have been called with the current camera state
    expect(mockUpdateCamera).toHaveBeenCalledWith('design-main', {
      position: [10, 20, 30],
      target: [5, 6, 7],
      up: [0, 1, 0],
      zoom: 2,
    });
  });

  it('does not throw and does not call updateCamera when viewportStore is absent', () => {
    render(() => <Viewport meshes={{}} viewportId="design-main" />);

    // Fire all captured 'end' listeners — must not throw (no store, no-op)
    expect(() => {
      for (const cb of controlsListeners['end'] ?? []) {
        cb();
      }
    }).not.toThrow();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FEA-mode wiring (step-23 — RED)
// Verifies that:
//   (a) feaModeStore prop causes the FeaModeToolbar to appear in the DOM
//   (b) absent feaModeStore → toolbar NOT rendered
//   (c) enabling the store calls meshManager.setColorize with the right MeshColorize
//   (d) disabling the store calls setColorize(null) then rebuildMaterials()
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport FEA wiring', () => {
  it('(a) renders fea-mode-toolbar when feaModeStore prop is provided', () => {
    const store = createFeaModeStore();
    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);
    expect(screen.getByTestId('fea-mode-toolbar')).toBeTruthy();
  });

  it('(b) does NOT render fea-mode-toolbar when feaModeStore prop is absent', () => {
    render(() => <Viewport meshes={{}} viewportId="test-vp" />);
    expect(screen.queryByTestId('fea-mode-toolbar')).toBeNull();
  });

  it('(c) setColorize called with correct MeshColorize when feaModeStore.enabled toggles true', () => {
    const store = createFeaModeStore(); // enabled=false initially
    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);

    // Clear any calls triggered during initial-mount effect (enabled=false → setColorize(null))
    mockMeshSetColorize.mockClear();
    mockBakeColours.mockClear();

    // Toggle enabled → true
    store.setEnabled(true);

    expect(mockMeshSetColorize).toHaveBeenCalledTimes(1);
    const colorize = mockMeshSetColorize.mock.calls[0][0];
    expect(colorize).not.toBeNull();
    expect(colorize.channel).toBe(store.state.channel); // 'vonMises'

    // Bake closure delegates to bakeColours with current range and palette
    const testScalars = new Float32Array([0.1, 0.5, 0.9]);
    colorize.bake(testScalars);
    expect(mockBakeColours).toHaveBeenCalledTimes(1);
    expect(mockBakeColours.mock.calls[0][0]).toBe(testScalars);
    expect(mockBakeColours.mock.calls[0][1]).toEqual({ mode: 'auto', min: 0, max: 1 });
    expect(mockBakeColours.mock.calls[0][2]).toBe(store.state.palette); // 'viridis'
  });

  it('(d) setColorize(null) then rebuildMaterials when feaModeStore.enabled toggles false', () => {
    const store = createFeaModeStore();
    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);

    // Enable first so we have a baseline
    store.setEnabled(true);

    // Clear mocks before the critical toggle
    mockMeshSetColorize.mockClear();
    mockMeshRebuildMaterials.mockClear();

    // Toggle enabled → false
    store.setEnabled(false);

    expect(mockMeshSetColorize).toHaveBeenCalledWith(null);
    expect(mockMeshRebuildMaterials).toHaveBeenCalledTimes(1);

    // Verify order: setColorize(null) must be called before rebuildMaterials
    const setColorizeOrder = mockMeshSetColorize.mock.invocationCallOrder[0];
    const rebuildOrder = mockMeshRebuildMaterials.mock.invocationCallOrder[0];
    expect(setColorizeOrder).toBeLessThan(rebuildOrder);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FEA auto-enable (step-25 — RED)
// Verifies that:
//   (a) first mesh arrival with non-empty scalar_channels triggers tryAutoEnable
//   (b) after user disable, subsequent scalar-channel mesh does NOT re-enable
//   (c) meshes with no scalar_channels or empty channels do NOT trigger auto-enable
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport FEA auto-enable', () => {
  it('(a) first mesh with non-empty scalar_channels auto-enables and sets channel', () => {
    const store = createFeaModeStore();
    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});

    render(() => <Viewport meshes={meshes()} viewportId="test-vp" feaModeStore={store as any} />);

    // No meshes yet — should not be auto-enabled
    expect(store.state.enabled).toBe(false);
    expect(store.state.autoEnabledOnce).toBe(false);

    // Deliver first mesh with a non-empty scalar channel
    setMeshes({
      'bracket': {
        entity_path: 'bracket',
        vertices: new Float32Array([0, 0, 0]),
        indices: new Uint32Array([0]),
        normals: null,
        scalar_channels: { vonMises: new Float32Array([0.5]) },
      },
    });

    // tryAutoEnable should have fired: enabled true, channel 'vonMises'
    expect(store.state.enabled).toBe(true);
    expect(store.state.autoEnabledOnce).toBe(true);
    expect(store.state.channel).toBe('vonMises');
  });

  it('(b) tryAutoEnable does NOT re-enable after user manually disables (one-shot sticky)', () => {
    const store = createFeaModeStore();
    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});

    render(() => <Viewport meshes={meshes()} viewportId="test-vp" feaModeStore={store as any} />);

    // First auto-enable
    setMeshes({
      'bracket': {
        entity_path: 'bracket',
        vertices: new Float32Array([0, 0, 0]),
        indices: new Uint32Array([0]),
        normals: null,
        scalar_channels: { vonMises: new Float32Array([0.5]) },
      },
    });
    expect(store.state.enabled).toBe(true);

    // User manually disables
    store.setEnabled(false);
    expect(store.state.enabled).toBe(false);

    // Subsequent mesh update with scalar_channels — should NOT re-enable
    setMeshes({
      'bracket2': {
        entity_path: 'bracket2',
        vertices: new Float32Array([1, 1, 1]),
        indices: new Uint32Array([0]),
        normals: null,
        scalar_channels: { vonMises: new Float32Array([0.9]) },
      },
    });

    // autoEnabledOnce sticky — user disable preserved
    expect(store.state.enabled).toBe(false);
  });

  it('(c) mesh without scalar_channels does NOT trigger auto-enable', () => {
    const store = createFeaModeStore();

    render(() => <Viewport
      meshes={{
        'bracket': {
          entity_path: 'bracket',
          vertices: new Float32Array([0, 0, 0]),
          indices: new Uint32Array([0]),
          normals: null,
          // No scalar_channels field
        },
      }}
      viewportId="test-vp"
      feaModeStore={store as any}
    />);

    expect(store.state.enabled).toBe(false);
    expect(store.state.autoEnabledOnce).toBe(false);
  });

  it('(c2) mesh with all-empty scalar_channels does NOT trigger auto-enable', () => {
    const store = createFeaModeStore();

    render(() => <Viewport
      meshes={{
        'bracket': {
          entity_path: 'bracket',
          vertices: new Float32Array([0, 0, 0]),
          indices: new Uint32Array([0]),
          normals: null,
          scalar_channels: { vonMises: new Float32Array(0) }, // Empty array
        },
      }}
      viewportId="test-vp"
      feaModeStore={store as any}
    />);

    expect(store.state.enabled).toBe(false);
    expect(store.state.autoEnabledOnce).toBe(false);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Viewport deformation bridge (step-21 — RED)
// Verifies that Viewport.tsx wires feaModeStore.showDeformed / warpFactor into
// meshManager.setDeformation via a SolidJS createEffect.
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport deformation bridge', () => {
  it('(a) showDeformed=true + warpFactor=5 calls setDeformation({warpFactor:5})', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    store.setShowDeformed(true);
    store.setWarpFactor(5);

    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);

    // The mount effect should have fired setDeformation with current warp factor.
    expect(mockMeshSetDeformation).toHaveBeenCalledWith({ warpFactor: 5 });
  });

  it('(b) setShowDeformed(false) after render calls setDeformation(null)', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    store.setShowDeformed(true);
    store.setWarpFactor(5);

    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);
    mockMeshSetDeformation.mockClear();

    store.setShowDeformed(false);

    expect(mockMeshSetDeformation).toHaveBeenCalledWith(null);
  });

  it('(c) changing warpFactor while showDeformed=true calls setDeformation({warpFactor:10})', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    store.setShowDeformed(true);
    store.setWarpFactor(5);

    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);
    mockMeshSetDeformation.mockClear();

    store.setWarpFactor(10);

    expect(mockMeshSetDeformation).toHaveBeenCalledWith({ warpFactor: 10 });
  });

  it('(d) changing warpFactor while showDeformed=false does NOT call setDeformation', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    // showDeformed remains false (default)

    render(() => <Viewport meshes={{}} viewportId="test-vp" feaModeStore={store as any} />);
    mockMeshSetDeformation.mockClear();

    // Change warpFactor — should NOT re-run the effect since showDeformed is false.
    store.setWarpFactor(99);

    // The effect must not re-run at all when showDeformed=false.
    expect(mockMeshSetDeformation).not.toHaveBeenCalled();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Viewport debug bridge map registration (step-1 — RED)
// Verifies that:
//   (a) mounting a Viewport registers it in window.__REIFY_DEBUG__.viewports[id]
//   (b) two Viewports with different ids both register; unmounting one leaves
//       the other intact in the map
// Both tests fail because the current code writes to viewport (singular), not
// the viewports map.
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport debug bridge map registration', () => {
  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('(a) registers viewport under its viewportId key in window.__REIFY_DEBUG__.viewports', () => {
    window.__REIFY_DEBUG__ = { stores: {} as any };
    render(() => <Viewport meshes={{}} viewportId="vp-A" />);

    const viewports = window.__REIFY_DEBUG__?.viewports;
    expect(viewports).toBeDefined();
    const vp = viewports!['vp-A'];
    expect(vp).toBeDefined();
    // Must expose the DebugViewport interface members
    expect(vp.scene).toBeDefined();
    expect(vp.camera).toBeDefined();
    expect(vp.renderer).toBeDefined();
    expect(typeof vp.getMeshes).toBe('function');
    expect(typeof vp.fitToView).toBe('function');
  });

  it('(b) two viewports with different ids both register; unmounting one leaves the other', () => {
    window.__REIFY_DEBUG__ = { stores: {} as any };

    const { unmount: unmountA } = render(() => <Viewport meshes={{}} viewportId="vp-A" />);
    render(() => <Viewport meshes={{}} viewportId="vp-B" />);

    // Both must be present
    expect(window.__REIFY_DEBUG__?.viewports?.['vp-A']).toBeDefined();
    expect(window.__REIFY_DEBUG__?.viewports?.['vp-B']).toBeDefined();

    // Unmount vp-A — its key must disappear
    unmountA();
    expect(window.__REIFY_DEBUG__?.viewports?.['vp-A']).toBeUndefined();

    // vp-B must survive
    expect(window.__REIFY_DEBUG__?.viewports?.['vp-B']).toBeDefined();
  });
});

// ── T0b: wireManager integration ──────────────────────────────────────────────

describe('Viewport wireManager integration (T0b)', () => {
  // RED until Viewport.tsx creates a wireManager and drives wireManager.sync.

  it('rendering Viewport with tensegrityWires calls wireManager.sync', () => {
    const wires = [
      { entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0 },
    ];
    render(() => <Viewport meshes={{}} viewportId="test-wm-vp" tensegrityWires={wires} />);
    // wireManager.sync must have been called (at least once via createEffect on mount).
    expect(mockWireSync).toHaveBeenCalled();
    const lastCall = mockWireSync.mock.calls[mockWireSync.mock.calls.length - 1];
    expect(lastCall[0]).toEqual(wires);
  });

  it('updating tensegrityWires prop re-syncs the wireManager', () => {
    const [wires, setWires] = createSignal([
      { entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0 },
    ]);
    render(() => <Viewport meshes={{}} viewportId="test-wm-vp2" tensegrityWires={wires()} />);
    const callCountAfterMount = mockWireSync.mock.calls.length;

    // Update the prop to an empty array.
    setWires([]);

    // sync must have been called again after the prop update.
    expect(mockWireSync.mock.calls.length).toBeGreaterThan(callCountAfterMount);
    const lastCall = mockWireSync.mock.calls[mockWireSync.mock.calls.length - 1];
    expect(lastCall[0]).toEqual([]);
  });

  it('unmounting Viewport disposes the wireManager', () => {
    const { unmount } = render(() => <Viewport meshes={{}} viewportId="test-wm-vp3" tensegrityWires={[]} />);
    mockWireDispose.mockClear();

    unmount();

    expect(mockWireDispose).toHaveBeenCalled();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Viewport FEA auto-range (step-5 RED — task 2962)
// Verifies that Viewport computes the active channel's data range and writes it
// into feaModeStore.range when mode === 'auto'.
//
// Tests fail until step-6 adds the auto-range createEffect and activeScalarRange
// memo to Viewport.tsx.
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport FEA auto-range', () => {
  it('(a) enabled + auto mode + meshes with vonMises [1,2,3] → range becomes {mode:auto,min:1,max:3}', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    // Initial range is {mode:'auto', min:0, max:1} (sentinel)
    expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-ar-vp" feaModeStore={store as any} />);

    setMeshes({ bracket: makeFEAMesh([1, 2, 3]) });

    // After delivering meshes, the auto-range effect must have updated the store.
    expect(store.state.range).toEqual({ mode: 'auto', min: 1, max: 3 });
  });

  it('(a2) bake closure receives the updated auto range {min:1,max:3} not the sentinel {0,1}', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-ar-bake" feaModeStore={store as any} />);

    setMeshes({ bracket: makeFEAMesh([1, 2, 3]) });
    mockBakeColours.mockClear();

    // Trigger a bake by toggling colorize (force re-set via channel change)
    const colorize = mockMeshSetColorize.mock.calls[mockMeshSetColorize.mock.calls.length - 1]?.[0];
    // If colorize is undefined, setColorize was never called — that is itself a regression.
    expect(colorize).toBeTruthy();
    if (colorize) {
      colorize.bake(new Float32Array([1, 2, 3]));
      expect(mockBakeColours.mock.calls[0][1]).toEqual({ mode: 'auto', min: 1, max: 3 });
    }
  });

  it('(b) empty meshes {} → range stays {auto,0,1} (no spurious setRange write)', () => {
    // Preserves existing Viewport.test.tsx:735 assertion.
    const store = createFeaModeStore();
    store.setEnabled(true);
    expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });

    render(() => <Viewport meshes={{}} viewportId="test-ar-empty" feaModeStore={store as any} />);

    // Range must stay at sentinel — computeScalarRange({}) returns null, so no write.
    expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });
  });

  it('(c) auto rescale: meshes A [1,3] then meshes B [10,20] → range updates to {auto,10,20}', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-ar-rescale" feaModeStore={store as any} />);

    // First delivery
    setMeshes({ bracket: makeFEAMesh([1, 3]) });
    expect(store.state.range).toEqual({ mode: 'auto', min: 1, max: 3 });

    // Second delivery with different data
    setMeshes({ bracket: makeFEAMesh([10, 20]) });
    expect(store.state.range).toEqual({ mode: 'auto', min: 10, max: 20 });
  });

  it('(d) locked survives re-solve: lock {1,3} then deliver [10,20] → range stays {locked,1,3}', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-ar-lock" feaModeStore={store as any} />);

    // Auto-range first
    setMeshes({ bracket: makeFEAMesh([1, 3]) });
    expect(store.state.range).toEqual({ mode: 'auto', min: 1, max: 3 });

    // User locks
    store.lockCurrent(1, 3);
    expect(store.state.range.mode).toBe('locked');

    // New solve result — locked range must NOT be overwritten
    setMeshes({ bracket: makeFEAMesh([10, 20]) });
    expect(store.state.range).toMatchObject({ mode: 'locked', min: 1, max: 3 });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Viewport FEA Lock Current + readout wiring (step-7 RED — task 2962)
//
// Tests fail until step-8 wires onLockCurrent and maxValue into <FeaModeToolbar>.
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport FEA Lock Current + readout wiring', () => {
  it('(a) clicking fea-mode-lock-current sets range to {locked, min:2, max:8}', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-lc-a" feaModeStore={store as any} />);

    setMeshes({ bracket: makeFEAMesh([2, 5, 8]) });

    // After auto-range, range should be {auto,2,8}
    expect(store.state.range).toEqual({ mode: 'auto', min: 2, max: 8 });

    // Click Lock current button
    const btn = screen.getByTestId('fea-mode-lock-current');
    fireEvent.click(btn);

    // Range must now be locked
    expect(store.state.range).toMatchObject({ mode: 'locked', min: 2, max: 8, source: 'current' });
  });

  it('(b) clicking Lock current with no FEA data (empty meshes) is a no-op', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    render(() => <Viewport meshes={{}} viewportId="test-lc-b" feaModeStore={store as any} />);

    // No FEA data — activeScalarRange() is null
    const btn = screen.getByTestId('fea-mode-lock-current');
    fireEvent.click(btn);

    // Range must stay at the auto sentinel (no lock happened)
    expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });
    expect(store.state.range.mode).not.toBe('locked');
  });

  it('(c) readout wiring: meshes vonMises [2,8] + enabled → toolbar shows fea-mode-max-readout with max=8', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);

    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});
    render(() => <Viewport meshes={meshes()} viewportId="test-lc-c" feaModeStore={store as any} />);

    setMeshes({ bracket: makeFEAMesh([2, 8]) });

    // Viewport must pass maxValue=8 down to FeaModeToolbar
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout).toBeTruthy();
    // Content must include the max value (8) in some numeric form
    expect(readout.textContent).toMatch(/8/);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Viewport FEA auto-enable determinism (step-3 — RED)
// Verifies that the auto-enable effect picks a channel deterministically
// regardless of the insertion order of scalar_channels keys.
//
// Against the current inline-loop implementation this is RED: the loop picks
// the first key in insertion order, which is 'vonMises_bottom' for test-1 and
// 'vonMises_top' for test-2 — different results for the same logical set.
// After step-4 wires pickDefaultScalarChannel, both cases stabilize on
// 'vonMises_top' (the PREFERRED_FEA_CHANNELS second entry, highest preference
// among the three shell channels).
// ─────────────────────────────────────────────────────────────────────────────
describe('Viewport FEA auto-enable determinism', () => {
  it('(test-1) channels inserted as {vonMises_bottom, vonMises_mid, vonMises_top} → channel is vonMises_top', () => {
    const store = createFeaModeStore();
    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});

    render(() => <Viewport meshes={meshes()} viewportId="test-det-1" feaModeStore={store as any} />);

    // Deliver mesh with channels in non-preferred insertion order
    setMeshes({
      shell: {
        entity_path: 'shell',
        vertices: new Float32Array([0, 0, 0]),
        indices: new Uint32Array([0]),
        normals: null,
        scalar_channels: {
          vonMises_bottom: new Float32Array([1]),
          vonMises_mid: new Float32Array([2]),
          vonMises_top: new Float32Array([3]),
        },
      },
    });

    expect(store.state.enabled).toBe(true);
    expect(store.state.channel).toBe('vonMises_top');
  });

  it('(test-2) channels inserted in reversed order {vonMises_top, vonMises_mid, vonMises_bottom} → still vonMises_top', () => {
    const store = createFeaModeStore();
    const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({});

    render(() => <Viewport meshes={meshes()} viewportId="test-det-2" feaModeStore={store as any} />);

    // Same channels, reversed insertion order
    setMeshes({
      shell: {
        entity_path: 'shell',
        vertices: new Float32Array([0, 0, 0]),
        indices: new Uint32Array([0]),
        normals: null,
        scalar_channels: {
          vonMises_top: new Float32Array([3]),
          vonMises_mid: new Float32Array([2]),
          vonMises_bottom: new Float32Array([1]),
        },
      },
    });

    expect(store.state.enabled).toBe(true);
    expect(store.state.channel).toBe('vonMises_top');
  });
});

// ── β: surfaceManager integration ─────────────────────────────────────────────
//
// Verifies that Viewport.tsx creates a surfaceManager and drives surfaceManager.sync
// reactively from props.tensegritySurfaces.
//
// RED until Viewport.tsx adds createSurfaceManager + the reactive sync effect
// (step-12).
// ─────────────────────────────────────────────────────────────────────────────

describe('Viewport surfaceManager integration (β)', () => {
  function makeSurface(overrides?: Partial<TensegritySurfaceData>): TensegritySurfaceData {
    return {
      entity_path: 'Patch',
      kind: 'membrane',
      i0: 0, i1: 1, i2: 2,
      x0: 0.0, y0: 0.0, z0: 0.0,
      x1: 1.0, y1: 0.0, z1: 0.0,
      x2: 0.5, y2: 0.866, z2: 0.0,
      ...overrides,
    };
  }

  it('rendering Viewport with tensegritySurfaces calls surfaceManager.sync', () => {
    const surfaces: TensegritySurfaceData[] = [makeSurface()];
    render(() => <Viewport meshes={{}} viewportId="test-sm-vp" tensegritySurfaces={surfaces} />);
    // surfaceManager.sync must have been called at least once via createEffect on mount.
    expect(mockSurfaceSync).toHaveBeenCalled();
    const lastCall = mockSurfaceSync.mock.calls[mockSurfaceSync.mock.calls.length - 1];
    expect(lastCall[0]).toEqual(surfaces);
  });

  it('updating tensegritySurfaces prop re-syncs the surfaceManager', () => {
    const [surfaces, setSurfaces] = createSignal<TensegritySurfaceData[]>([makeSurface()]);
    render(() => <Viewport meshes={{}} viewportId="test-sm-vp2" tensegritySurfaces={surfaces()} />);
    const callCountAfterMount = mockSurfaceSync.mock.calls.length;

    // Update the prop to an empty array.
    setSurfaces([]);

    // sync must have been called again after the prop update.
    expect(mockSurfaceSync.mock.calls.length).toBeGreaterThan(callCountAfterMount);
    const lastCall = mockSurfaceSync.mock.calls[mockSurfaceSync.mock.calls.length - 1];
    expect(lastCall[0]).toEqual([]);
  });

  it('unmounting Viewport disposes the surfaceManager', () => {
    const { unmount } = render(() => <Viewport meshes={{}} viewportId="test-sm-vp3" tensegritySurfaces={[]} />);
    mockSurfaceDispose.mockClear();

    unmount();

    expect(mockSurfaceDispose).toHaveBeenCalled();
  });
});
