import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@solidjs/testing-library';

// Stub ResizeObserver for jsdom (which doesn't support it)
globalThis.ResizeObserver = class ResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: ResizeObserverCallback) {}
};

// Stub requestAnimationFrame/cancelAnimationFrame
globalThis.requestAnimationFrame = vi.fn((cb) => setTimeout(cb, 0) as unknown as number);
globalThis.cancelAnimationFrame = vi.fn((id) => clearTimeout(id));

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

import { Viewport } from '../../viewport';

beforeEach(() => {
  vi.clearAllMocks();
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
});
