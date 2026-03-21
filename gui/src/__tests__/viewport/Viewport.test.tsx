import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';

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

vi.mock('../../viewport/controls', () => ({
  createControls: vi.fn(() => ({
    controls: {},
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
const mockSelectionDispose = vi.fn();

vi.mock('../../viewport/selection', () => ({
  createSelection: vi.fn(() => ({
    setHovered: mockSelectionSetHovered,
    setSelected: mockSelectionSetSelected,
    fitToView: mockSelectionFitToView,
    dispose: mockSelectionDispose,
  })),
}));

import { Viewport } from '../../viewport';

beforeEach(() => {
  vi.clearAllMocks();
  rafCallbacks = [];
  rafIdCounter = 1;
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
});
