// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock three.js before importing anything that uses it
const mockSetClearColor = vi.fn();
const mockSetSize = vi.fn();
const mockSetPixelRatio = vi.fn();
const mockRendererDispose = vi.fn();

const mockSceneAdd = vi.fn();
const mockSceneChildren: any[] = [];
const mockCameraAdd = vi.fn();

vi.mock('three', () => {
  class MockScene {
    children = mockSceneChildren;
    add = mockSceneAdd;
    background = null;
  }

  class MockPerspectiveCamera {
    fov: number;
    aspect: number;
    near: number;
    far: number;
    position = { set: vi.fn() };
    updateProjectionMatrix = vi.fn();
    add = mockCameraAdd;
    constructor(fov: number, aspect: number, near: number, far: number) {
      this.fov = fov;
      this.aspect = aspect;
      this.near = near;
      this.far = far;
    }
  }

  class MockWebGLRenderer {
    setClearColor = mockSetClearColor;
    setSize = mockSetSize;
    setPixelRatio = mockSetPixelRatio;
    dispose = mockRendererDispose;
    domElement = document.createElement('canvas');
    constructor(_opts?: any) {}
  }

  class MockAmbientLight {
    type = 'AmbientLight';
    intensity: number;
    constructor(_color?: any, intensity?: number) {
      this.intensity = intensity ?? 1;
    }
  }

  class MockDirectionalLight {
    type = 'DirectionalLight';
    intensity: number;
    position = { set: vi.fn() };
    constructor(_color?: any, intensity?: number) {
      this.intensity = intensity ?? 1;
    }
  }

  class MockGridHelper {
    type = 'GridHelper';
    constructor(public size?: number, public divisions?: number) {}
  }

  class MockAxesHelper {
    type = 'AxesHelper';
    constructor(public size?: number) {}
  }

  class MockColor {
    constructor(public color?: any) {}
  }

  return {
    Scene: MockScene,
    PerspectiveCamera: MockPerspectiveCamera,
    WebGLRenderer: MockWebGLRenderer,
    AmbientLight: MockAmbientLight,
    DirectionalLight: MockDirectionalLight,
    GridHelper: MockGridHelper,
    AxesHelper: MockAxesHelper,
    Color: MockColor,
  };
});

import { createScene } from '../../viewport/scene';

beforeEach(() => {
  vi.clearAllMocks();
  mockSceneChildren.length = 0;
});

describe('createScene', () => {
  function setup() {
    const canvas = document.createElement('canvas');
    return createScene(canvas, 800, 600);
  }

  it('returns object with scene, camera, renderer, and resize', () => {
    const result = setup();
    expect(result).toHaveProperty('scene');
    expect(result).toHaveProperty('camera');
    expect(result).toHaveProperty('renderer');
    expect(result).toHaveProperty('resize');
    expect(typeof result.resize).toBe('function');
  });

  it('creates PerspectiveCamera with reasonable defaults', () => {
    const { camera } = setup();
    // FOV should be reasonable (45-75)
    expect(camera.fov).toBeGreaterThanOrEqual(45);
    expect(camera.fov).toBeLessThanOrEqual(75);
    // Near/far should be set
    expect(camera.near).toBeGreaterThan(0);
    expect(camera.far).toBeGreaterThan(camera.near);
  });

  it('scene has AmbientLight and DirectionalLight added', () => {
    setup();
    // Check scene.add was called with light objects
    const addedTypes = mockSceneAdd.mock.calls.map((c: any) => c[0]?.type);
    expect(addedTypes).toContain('AmbientLight');
    expect(addedTypes).toContain('DirectionalLight');
  });

  it('renderer setClearColor was called with theme viewportBg color', () => {
    setup();
    expect(mockSetClearColor).toHaveBeenCalled();
    // The first argument should be a Color constructed with the viewportBg hex
    const colorArg = mockSetClearColor.mock.calls[0][0];
    expect(colorArg).toBeDefined();
  });

  it('scene contains GridHelper', () => {
    setup();
    const addedTypes = mockSceneAdd.mock.calls.map((c: any) => c[0]?.type);
    expect(addedTypes).toContain('GridHelper');
  });

  it('scene contains AxesHelper', () => {
    setup();
    const addedTypes = mockSceneAdd.mock.calls.map((c: any) => c[0]?.type);
    expect(addedTypes).toContain('AxesHelper');
  });

  it('resize updates camera aspect and renderer size', () => {
    const { camera, resize } = setup();
    resize(1024, 768);
    expect(camera.aspect).toBeCloseTo(1024 / 768);
    expect(camera.updateProjectionMatrix).toHaveBeenCalled();
    expect(mockSetSize).toHaveBeenCalledWith(1024, 768);
  });

  it('resize calls renderer.setPixelRatio with window.devicePixelRatio (V-15)', () => {
    const { resize } = setup();
    // Clear the initial setPixelRatio call from construction
    mockSetPixelRatio.mockClear();

    // Simulate a high-DPI display
    Object.defineProperty(window, 'devicePixelRatio', { value: 2, configurable: true });

    resize(1024, 768);

    expect(mockSetPixelRatio).toHaveBeenCalledWith(2);

    // Restore
    Object.defineProperty(window, 'devicePixelRatio', { value: 1, configurable: true });
  });

  it('adds a camera-following headlight via camera.add (V-13)', () => {
    const { camera } = setup();
    // A DirectionalLight should be added as a child of the camera
    const cameraChildren = mockCameraAdd.mock.calls.map((c: any) => c[0]);
    const headlight = cameraChildren.find((child: any) => child?.type === 'DirectionalLight');
    expect(headlight).toBeDefined();
    expect(headlight.intensity).toBeGreaterThan(0);
  });

  it('camera is added to scene so its children are rendered (V-13)', () => {
    setup();
    // scene.add should be called with the camera instance (has .fov property)
    const addedObjects = mockSceneAdd.mock.calls.map((c: any) => c[0]);
    const cameraInScene = addedObjects.find((obj: any) => obj?.fov !== undefined);
    expect(cameraInScene).toBeDefined();
  });
});
