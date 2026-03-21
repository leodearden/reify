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
const mockSelectionFitToView = vi.fn();
const mockSelectionFlyToEntity = vi.fn();
const mockSelectionInvalidateRect = vi.fn();
const mockSelectionDispose = vi.fn();

vi.mock('../../viewport/selection', () => ({
  createSelection: vi.fn(() => ({
    setHovered: mockSelectionSetHovered,
    setSelected: mockSelectionSetSelected,
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

    // Restore original ResizeObserver
    globalThis.ResizeObserver = OrigRO;
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
