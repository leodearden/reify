import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import type { MeshData } from '../../types';

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

const mockGrid = { type: 'GridHelper', visible: true };
const mockAxes = { type: 'AxesHelper', visible: true };

vi.mock('../../viewport/scene', () => ({
  createScene: vi.fn(() => ({
    scene: { type: 'Scene' },
    camera: { type: 'PerspectiveCamera' },
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
  })),
}));

// Captured event listeners from the controls mock (for render-on-demand tests)
let controlsListeners: Record<string, Function[]> = {};

vi.mock('../../viewport/controls', () => ({
  createControls: vi.fn(() => ({
    controls: {
      addEventListener: vi.fn((event: string, cb: Function) => {
        if (!controlsListeners[event]) controlsListeners[event] = [];
        controlsListeners[event].push(cb);
      }),
      removeEventListener: vi.fn((_event: string, _cb: Function) => {}),
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
});

describe('Viewport', () => {
  it('renders a canvas element with data-testid viewport-canvas', () => {
    render(() => <Viewport meshes={{}} />);
    expect(screen.getByTestId('viewport-canvas')).toBeTruthy();
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.tagName.toLowerCase()).toBe('canvas');
  });

  it('canvas is wrapped in a container div with data-testid viewport-container', () => {
    render(() => <Viewport meshes={{}} />);
    const container = screen.getByTestId('viewport-container');
    expect(container).toBeTruthy();
    expect(container.tagName.toLowerCase()).toBe('div');
    // Canvas should be inside the container
    const canvas = screen.getByTestId('viewport-canvas');
    expect(container.contains(canvas)).toBe(true);
  });

  it('shows tooltip with entity name when hoveredEntity is set', () => {
    render(() => <Viewport meshes={{}} hoveredEntity="bracket/hole" />);
    const tooltip = screen.getByTestId('viewport-tooltip');
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain('bracket/hole');
  });

  it('hides tooltip when hoveredEntity is null', () => {
    render(() => <Viewport meshes={{}} hoveredEntity={null} />);
    expect(screen.queryByTestId('viewport-tooltip')).toBeNull();
  });

  it('shows spinner overlay when evalStatus phase is evaluating', () => {
    render(() => <Viewport meshes={{}} evalStatus={{ phase: 'evaluating' }} />);
    const spinner = screen.getByTestId('viewport-spinner');
    expect(spinner).toBeTruthy();
  });

  it('hides spinner when evalStatus phase is idle', () => {
    render(() => <Viewport meshes={{}} evalStatus={{ phase: 'idle' }} />);
    expect(screen.queryByTestId('viewport-spinner')).toBeNull();
  });

  it('renders fit-to-view button with data-testid', () => {
    render(() => <Viewport meshes={{}} />);
    const btn = screen.getByTestId('fit-to-view');
    expect(btn).toBeTruthy();
  });

  it('hides spinner when evalStatus is not provided', () => {
    render(() => <Viewport meshes={{}} />);
    expect(screen.queryByTestId('viewport-spinner')).toBeNull();
  });

  it('clicking fit-to-view button calls selection.fitToView', async () => {
    const onFitToView = vi.fn();
    render(() => <Viewport meshes={{}} onFitToView={onFitToView} />);
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

    render(() => <Viewport meshes={meshes()} selectedEntity="bracket/plate" />);

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
    const { unmount } = render(() => <Viewport meshes={{}} />);

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
    render(() => <Viewport meshes={{}} flyToEntityRef={flyToEntityRef} />);
    expect(flyToEntityRef).toHaveBeenCalledTimes(1);
    expect(typeof flyToEntityRef.mock.calls[0][0]).toBe('function');
  });

  it('flyToEntityRef function delegates to selection.flyToEntity', () => {
    let capturedFn: ((entityPath: string) => void) | undefined;
    const flyToEntityRef = vi.fn((fn: (entityPath: string) => void) => {
      capturedFn = fn;
    });
    render(() => <Viewport meshes={{}} flyToEntityRef={flyToEntityRef} />);

    expect(capturedFn).toBeDefined();
    capturedFn!('Bracket');
    expect(mockSelectionFlyToEntity).toHaveBeenCalledWith('Bracket');
  });

  it('animate loop does NOT call renderer.render when idle after initial frame', () => {
    render(() => <Viewport meshes={{}} />);

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
    render(() => <Viewport meshes={{}} />);

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
      render(() => <Viewport meshes={{}} />);

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
    render(() => <Viewport meshes={{}} />);

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
    render(() => <Viewport meshes={{}} fitToViewRef={fitToViewRef} />);
    expect(fitToViewRef).toHaveBeenCalledTimes(1);
    expect(typeof fitToViewRef.mock.calls[0][0]).toBe('function');
  });

  it('fitToViewRef function delegates to selection.fitToView', () => {
    let capturedFn: (() => void) | undefined;
    const fitToViewRef = vi.fn((fn: () => void) => {
      capturedFn = fn;
    });
    render(() => <Viewport meshes={{}} fitToViewRef={fitToViewRef} />);

    expect(capturedFn).toBeDefined();
    mockSelectionFitToView.mockClear();
    capturedFn!();
    expect(mockSelectionFitToView).toHaveBeenCalled();
  });

  it('renders a grid toggle button with data-testid toggle-grid', () => {
    render(() => <Viewport meshes={{}} />);
    const btn = screen.getByTestId('toggle-grid');
    expect(btn).toBeTruthy();
  });

  it('clicking toggle-grid button toggles grid and axes visible state', () => {
    render(() => <Viewport meshes={{}} />);
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

  it('tooltip style top/left update on mousemove within container', () => {
    render(() => <Viewport meshes={{}} hoveredEntity="bracket/plate" />);
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
    render(() => <Viewport meshes={{}} hoveredEntity="bracket/plate" />);
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
    render(() => <Viewport meshes={{}} hoveredEntity="bracket/plate" />);
    const tooltip = screen.getByTestId('viewport-tooltip');
    expect(tooltip.textContent).toContain('bracket/plate');
  });
});

describe('Viewport accessibility', () => {
  it('canvas has tabindex="0" so it can receive keyboard focus', () => {
    render(() => <Viewport meshes={{}} />);
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.getAttribute('tabindex')).toBe('0');
  });

  it('canvas has aria-label="3D viewport"', () => {
    render(() => <Viewport meshes={{}} />);
    const canvas = screen.getByTestId('viewport-canvas');
    expect(canvas.getAttribute('aria-label')).toBe('3D viewport');
  });
});
