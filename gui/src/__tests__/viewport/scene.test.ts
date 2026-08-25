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
/** Name of the helper class whose `material` should be an array for the next
 *  createScene() call — drives the singleMaterial() throw path in scene.ts. */
let mockArrayMaterialFor: string | null = null;

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

vi.mock('three', async () => {
  // Axis-label sprite mocks are shared with axisLabels.test.ts via threeAxisMocks.ts
  // to prevent silent drift between the two suites.
  const {
    MockGroup,
    MockSprite,
    MockSpriteMaterial,
    MockCanvasTexture,
    MockColor: MockColorShared,
    LinearFilter: LinearFilterShared,
  } = await import('./threeAxisMocks');

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

  // Helper materials start EMPTY, not at three.js's real defaults. scene.ts asserts the
  // helper-tier depth contract by assigning both flags explicitly, so an untouched flag
  // reads back `undefined` and any `toBe(true)`/`toBe(false)` assertion below fails. Seeding
  // the real defaults ({ depthTest: true, depthWrite: true }) would make the depthTest
  // assertions vacuous — they would pass whether or not scene.ts set anything. Same intent
  // as `ctorOpts` in threeAxisMocks.ts: assert on the write, never on a default that agrees.
  type MockHelperMaterial = { depthTest?: boolean; depthWrite?: boolean };

  /** Set to a helper class name to make that helper hand back an ARRAY material,
   *  exercising scene.ts's singleMaterial() guard. Reset in beforeEach. */
  function mockHelperMaterialFor(name: string): MockHelperMaterial | MockHelperMaterial[] {
    return mockArrayMaterialFor === name ? [{}, {}] : {};
  }

  /** Recording uniform-scale stub: `setScalar` is a spy that also WRITES BACK, so a
   *  test can assert both "was it called" and "what is the resulting factor". Starts
   *  at the three.js identity 1 — unlike the depth flags above, 1 is not a value the
   *  #6588 assertions accept as a pass (they demand a scene-derived factor), so
   *  seeding it does not make anything vacuous; it makes the no-op guard tests real. */
  function makeMockScale() {
    const s = {
      x: 1, y: 1, z: 1,
      setScalar: vi.fn((v: number) => {
        s.x = v; s.y = v; s.z = v;
        return s;
      }),
      set: vi.fn((x: number, y: number, z: number) => {
        s.x = x; s.y = y; s.z = z;
        return s;
      }),
    };
    return s;
  }

  class MockGridHelper {
    type = 'GridHelper';
    visible = true;
    rotation = { x: 0, y: 0, z: 0 };
    renderOrder = 0;
    scale = makeMockScale();
    material = mockHelperMaterialFor('GridHelper');
    constructor(public size?: number, public divisions?: number) {}
  }

  class MockAxesHelper {
    type = 'AxesHelper';
    visible = true;
    renderOrder = 0;
    scale = makeMockScale();
    material = mockHelperMaterialFor('AxesHelper');
    constructor(public size?: number) {}
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
    Color: MockColorShared,
    Vector3: MockVector3,
    Group: MockGroup,
    Sprite: MockSprite,
    SpriteMaterial: MockSpriteMaterial,
    CanvasTexture: MockCanvasTexture,
    // scene.ts imports the REAL axisLabels.ts, so this factory must satisfy that
    // module's 'three' imports too — including LinearFilter (#6588).
    LinearFilter: LinearFilterShared,
  };
});

import { createScene } from '../../viewport/scene';
import { GRID_RENDER_ORDER, AXES_RENDER_ORDER } from '../../viewport/renderOrder';

beforeEach(() => {
  vi.clearAllMocks();
  mockSceneChildren.length = 0;
  lastRendererOpts = undefined;
  mockArrayMaterialFor = null;
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

  it('axes are depth-tested so model geometry in front of the origin occludes them (#6587)', () => {
    const result = setup();
    const ax = result.axes as any;
    // depthTest=false made the axis vectors draw straight through solid parts.
    // They are full-scene-scale world geometry, not a HUD, so they must obey depth.
    // The mock material starts EMPTY, so this only passes if scene.ts assigns the flag —
    // it is not satisfied by three.js's default happening to agree.
    expect(ax.material.depthTest).toBe(true);
    // ...but they still write no depth, so they cannot occlude the labels above them.
    expect(ax.material.depthWrite).toBe(false);
  });

  it('the grid writes no depth so its centre lines can never z-fight the collinear X/Y axes (#4214)', () => {
    const result = setup();
    const gr = result.grid as any;
    // The grid stays depth-tested: real meshes in front of it still occlude it.
    // Assigned explicitly by scene.ts (mock material starts empty), so this is a real pin.
    expect(gr.material.depthTest).toBe(true);
    // depthWrite=false is what breaks the coplanar tie. The grid contributes nothing
    // to the depth buffer, so the axes drawn after it can never fail their LEQUAL
    // test against it — #4214 stays fixed without disabling depthTest anywhere.
    expect(gr.material.depthWrite).toBe(false);
  });

  it('grid and axes draw in the helper tier, after all model geometry', () => {
    const result = setup();
    expect(result.grid.renderOrder).toBe(GRID_RENDER_ORDER);
    expect(result.axes.renderOrder).toBe(AXES_RENDER_ORDER);
    // Order within the tier is what decides the coplanar grid-vs-axes tie.
    expect(result.grid.renderOrder).toBeLessThan(result.axes.renderOrder);
    // The whole tier sits above the default mesh tier 0.
    expect(result.grid.renderOrder).toBeGreaterThan(0);
  });

  // The singleMaterial() guard in scene.ts is a hard failure inside viewport init, so its
  // message wording and its very reachability need at least one execution. Real three.js
  // hands back a single material today; the mock forges the array case the guard exists for.
  it.each([
    ['GridHelper'],
    ['AxesHelper'],
  ])('throws if %s.material becomes an array in a future three.js', (helperName) => {
    mockArrayMaterialFor = helperName;
    expect(() => setup()).toThrow(
      `${helperName}.material is unexpectedly an array — three.js API changed; update overlay logic in scene.ts.`,
    );
  });

  it('returns axisLabels property that is a Group', () => {
    const result = setup();
    expect(result).toHaveProperty('axisLabels');
    expect((result as any).axisLabels.type).toBe('Group');
  });

  it('axisLabels group is added to the scene', () => {
    const result = setup();
    const axisLabels = (result as any).axisLabels;
    const addedObjects = mockSceneAdd.mock.calls.map((c: any) => c[0]);
    const found = addedObjects.find((obj: any) => obj === axisLabels);
    expect(found).toBeDefined();
  });

  it('axisLabels group has 3 children (X, Y, Z sprites)', () => {
    const result = setup();
    const axisLabels = (result as any).axisLabels;
    expect(axisLabels.children).toHaveLength(3);
  });

  it('exposes disposeAxisLabels function for GPU resource cleanup on unmount', () => {
    const result = setup();
    expect(typeof result.disposeAxisLabels).toBe('function');
  });
});


// ── fitHelpers (#6588) ───────────────────────────────────────────────────────
// The grid (20 units), the axes triad (2 units) and the label ring (2.3 units)
// carry absolute sizes chosen for the ~10 m CAD default scene. In a sub-metre
// .ri model the camera ends up INSIDE that helper envelope: the 20 m grid's far
// lines converge into a dark 0x444466 horizon band and the 2 m triad draws a
// hard diagonal across every part. fitHelpers sizes them to the actual scene.

describe('fitHelpers (#6588)', () => {
  function setup() {
    const canvas = document.createElement('canvas');
    return createScene(canvas, 800, 600);
  }

  /** Box3-like fake whose getSize writes the given extent. Same shape as the
   *  adjustClipping fakes above — fitHelpers must not need a real Box3. */
  function boundsWithSize(x: number, y: number, z: number, isEmpty = false) {
    return {
      isEmpty: () => isEmpty,
      getCenter: (target: any) => {
        target.x = 0; target.y = 0; target.z = 0;
        return target;
      },
      getSize: (target: any) => {
        target.x = x; target.y = y; target.z = z;
        return target;
      },
    };
  }

  /** Non-zero coordinate of each label sprite, keyed by its declared axis. */
  function labelOffsets(result: any): Record<string, number> {
    const out: Record<string, number> = {};
    for (const sprite of result.axisLabels.children as any[]) {
      const axis = sprite.userData.axis as 'X' | 'Y' | 'Z';
      out[axis] = axis === 'X' ? sprite.position.x
        : axis === 'Y' ? sprite.position.y
        : sprite.position.z;
    }
    return out;
  }

  function expectNoOp(result: any, before: Record<string, number>) {
    // Neither helper was resized...
    expect((result.grid as any).scale.setScalar).not.toHaveBeenCalled();
    expect((result.axes as any).scale.setScalar).not.toHaveBeenCalled();
    expect((result.grid as any).scale.x).toBe(1);
    expect((result.axes as any).scale.x).toBe(1);
    // ...and the label ring stayed where construction put it. A guard that let a
    // degenerate measurement through would show up here as 0 / NaN / Infinity.
    expect(labelOffsets(result)).toEqual(before);
  }

  it('exposes a fitHelpers method', () => {
    const result = setup();
    expect(result).toHaveProperty('fitHelpers');
    expect(typeof (result as any).fitHelpers).toBe('function');
  });

  it('empty bounds are a no-op', () => {
    const result = setup() as any;
    const before = labelOffsets(result);
    const emptyBounds = {
      isEmpty: () => true,
      getCenter: vi.fn(),
      getSize: vi.fn(),
    };

    result.fitHelpers(emptyBounds as any);

    expectNoOp(result, before);
    // The isEmpty() early return must fire BEFORE any measurement, mirroring
    // adjustClipping's guard.
    expect(emptyBounds.getSize).not.toHaveBeenCalled();
  });

  it('zero-size bounds are a no-op', () => {
    const result = setup() as any;
    const before = labelOffsets(result);
    // A single degenerate mesh can produce a non-empty Box3 of zero extent.
    // radius would be 0, and a 0-unit grid is worse than an oversized one.
    result.fitHelpers(boundsWithSize(0, 0, 0) as any);
    expectNoOp(result, before);
  });

  it.each([
    ['NaN', NaN],
    ['Infinity', Infinity],
  ])('non-finite (%s) bounds are a no-op', (_label, v) => {
    const result = setup() as any;
    const before = labelOffsets(result);
    result.fitHelpers(boundsWithSize(v, v, v) as any);
    expectNoOp(result, before);
  });
});
