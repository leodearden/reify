// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock three.js before importing anything that uses it
const mockSetClearColor = vi.fn();
const mockSetSize = vi.fn();
const mockSetPixelRatio = vi.fn();
const mockRendererDispose = vi.fn();
let lastRendererOpts: any;

const mockSceneAdd = vi.fn();
const mockSceneChildren: any[] = [];
const mockCameraAdd = vi.fn();

function makeMockVector3() {
  const v = {
    x: 0, y: 0, z: 0,
    set: vi.fn((x: number, y: number, z: number) => {
      v.x = x; v.y = y; v.z = z;
    }),
    distanceTo: vi.fn((target: any) => {
      const dx = v.x - target.x;
      const dy = v.y - target.y;
      const dz = v.z - target.z;
      return Math.sqrt(dx * dx + dy * dy + dz * dz);
    }),
  };
  return v;
}

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
    position = makeMockVector3();
    up = makeMockVector3();
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
    constructor(opts?: any) { lastRendererOpts = opts; }
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
    visible = true;
    rotation = { x: 0, y: 0, z: 0 };
    renderOrder = 0;
    material = { depthTest: true, depthWrite: true };
    constructor(public size?: number, public divisions?: number) {}
  }

  class MockAxesHelper {
    type = 'AxesHelper';
    visible = true;
    renderOrder = 0;
    material = { depthTest: true, depthWrite: true };
    constructor(public size?: number) {}
  }

  class MockColor {
    constructor(public color?: any) {}
  }

  class MockVector3 {
    x: number;
    y: number;
    z: number;
    constructor(x = 0, y = 0, z = 0) {
      this.x = x; this.y = y; this.z = z;
    }
    length() {
      return Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z);
    }
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
    Vector3: MockVector3,
  };
});

import { createScene } from '../../viewport/scene';

beforeEach(() => {
  vi.clearAllMocks();
  mockSceneChildren.length = 0;
  lastRendererOpts = undefined;
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

  it('exposes adjustClipping method (V-11)', () => {
    const result = setup();
    expect(result).toHaveProperty('adjustClipping');
    expect(typeof result.adjustClipping).toBe('function');
  });

  it('adjustClipping updates camera.near, camera.far and calls updateProjectionMatrix (V-11)', () => {
    const { camera, adjustClipping } = setup();
    vi.mocked(camera.updateProjectionMatrix).mockClear();

    // Mock a Box3-like bounds object: center at (10, 10, 10), size 20x20x20
    const bounds = {
      isEmpty: () => false,
      getCenter: (target: any) => {
        target.x = 10; target.y = 10; target.z = 10;
        return target;
      },
      getSize: (target: any) => {
        target.x = 20; target.y = 20; target.z = 20;
        return target;
      },
    };

    // Camera is at (5,5,5) by default via position.set mock
    // We need the camera.position to be readable for distance computation
    camera.position.x = 5;
    camera.position.y = 5;
    camera.position.z = 5;

    adjustClipping(bounds as any);

    // near should be > 0 and less than far
    expect(camera.near).toBeGreaterThan(0);
    expect(camera.far).toBeGreaterThan(camera.near);
    expect(camera.updateProjectionMatrix).toHaveBeenCalled();
  });

  it('returns grid property that is a GridHelper instance', () => {
    const result = setup();
    expect(result).toHaveProperty('grid');
    expect(result.grid.type).toBe('GridHelper');
  });

  it('returns axes property that is an AxesHelper instance', () => {
    const result = setup();
    expect(result).toHaveProperty('axes');
    expect(result.axes.type).toBe('AxesHelper');
  });

  it('grid and axes have a visible property (initially true)', () => {
    const result = setup();
    expect(result.grid).toHaveProperty('visible');
    expect(result.axes).toHaveProperty('visible');
  });

  it('sets camera.up to (0, 0, 1) — Z-up convention to match reify kernel', () => {
    const { camera } = setup();
    // Use toHaveBeenLastCalledWith so the assertion pins the *final* call even
    // if upstream code called set() more than once (guards against later overrides).
    expect((camera.up as any).set).toHaveBeenLastCalledWith(0, 0, 1);
    // Assert the full triple so a stray set(0,0,0) after the correct call cannot pass.
    expect((camera.up as any).x).toBe(0);
    expect((camera.up as any).y).toBe(0);
    expect((camera.up as any).z).toBe(1);
  });

  it('rotates GridHelper onto the XY plane (rotation.x = π/2) so the grid is the floor under Z-up', () => {
    const result = setup();
    expect(result.grid.rotation.x).toBeCloseTo(Math.PI / 2);
    expect(result.grid.rotation.y).toBe(0);
    expect(result.grid.rotation.z).toBe(0);
  });

  it('constructs WebGLRenderer with preserveDrawingBuffer: true (required for html-to-image full-window capture)', () => {
    setup();
    expect(lastRendererOpts).toBeDefined();
    expect(lastRendererOpts.preserveDrawingBuffer).toBe(true);
  });

  it('adjustClipping with empty bounds is a no-op (V-11)', () => {
    const { camera, adjustClipping } = setup();
    const origNear = camera.near;
    const origFar = camera.far;
    vi.mocked(camera.updateProjectionMatrix).mockClear();

    const emptyBounds = {
      isEmpty: () => true,
      getCenter: vi.fn(),
      getSize: vi.fn(),
    };

    adjustClipping(emptyBounds as any);

    // Should not modify clipping planes
    expect(camera.near).toBe(origNear);
    expect(camera.far).toBe(origFar);
    expect(camera.updateProjectionMatrix).not.toHaveBeenCalled();
  });

  it('axes draw after the grid (renderOrder) so the grid cannot occlude them', () => {
    const result = setup();
    expect(result.axes.renderOrder).toBe(1);
    expect(result.axes.renderOrder).toBeGreaterThan(result.grid.renderOrder);
    // Pin the grid's own renderOrder at the default so a regression that also
    // mutated the grid would be caught (the fix relies on the grid staying at 0).
    expect(result.grid.renderOrder).toBe(0);
  });

  it('axes ignore the depth buffer so coplanar grid lines never z-fight over them', () => {
    const result = setup();
    const ax = result.axes as any;
    expect(ax.material.depthTest).toBe(false);
    expect(ax.material.depthWrite).toBe(false);
    // Pin the grid's depth flags at defaults — the fix depends on the grid keeping
    // normal depthTest/depthWrite so real 3D meshes still occlude it correctly.
    const gr = result.grid as any;
    expect(gr.material.depthTest).toBe(true);
    expect(gr.material.depthWrite).toBe(true);
  });
});
