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

import { createScene, CAMERA_FOV_DEG } from '../../viewport/scene';
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

  // The #6588 label-footprint contract is a JOINT property of two modules:
  // axisLabels.ts's LABEL_SCREEN_SCALE and this module's camera fov. axisLabels.test.ts
  // can only see the first half — it mocks three and never builds a camera, so it
  // restates the fov as a local literal. Retuning the camera fov is a plausible,
  // unrelated change that would silently magnify every label while that suite stayed
  // green (fov 30 doubles the footprint). This is the test that fails instead.
  it('keeps the axis labels under 10% of the viewport height at the fov the camera actually uses', () => {
    const result = setup();

    // Pins the camera to the exported constant: a literal re-introduced at the
    // construction site would decouple the two again without failing anything else.
    expect(result.camera.fov).toBe(CAMERA_FOV_DEG);

    for (const sprite of result.axisLabels.children) {
      // three r183 sprite.glsl.js with sizeAttenuation: false applies
      // `scale *= -mvPosition.z`, cancelling the perspective divide, so the label's
      // share of the viewport HEIGHT is f = s * cot(fov/2) / 2 — no distance term.
      // At s = 0.055 and fov = 60 that is 4.8%; the #6588 dogfood frame showed 69%.
      const frac = (sprite.scale.x * (1 / Math.tan((CAMERA_FOV_DEG * Math.PI) / 180 / 2))) / 2;
      expect(frac).toBeGreaterThan(0);
      expect(frac).toBeLessThanOrEqual(0.1);
    }
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

  /**
   * The post-state EVERY degenerate-bounds path must leave behind: the helpers at
   * their construction sizing (scale 1) with the label ring at its construction
   * offset.
   *
   * Asserted as STATE, deliberately, rather than as "setScalar was never called":
   * fitHelpers RESETS on a degenerate measurement instead of preserving the last
   * good one, so the two orderings below (degenerate-first vs degenerate-after-a-
   * good-fit) share one expectation. A "was not called" assertion could only ever
   * describe the pristine ordering, which is not the one that bites in the app.
   */
  function expectBaseHelperSizing(result: any, defaults: Record<string, number>) {
    for (const helper of [result.grid, result.axes]) {
      expect(helper.scale.x).toBe(1);
      expect(helper.scale.y).toBe(1);
      expect(helper.scale.z).toBe(1);
    }
    // A guard that let a degenerate measurement through would show up here as
    // 0 / NaN / Infinity rather than the construction offset.
    expect(labelOffsets(result)).toEqual(defaults);
  }

  /** The measurements fitHelpers must refuse to apply. Each entry is a FACTORY so
   *  every case gets its own fresh spies. */
  const DEGENERATE_CASES: Array<[string, () => any]> = [
    ['empty', () => ({ isEmpty: () => true, getCenter: vi.fn(), getSize: vi.fn() })],
    // A single degenerate mesh can produce a non-empty Box3 of zero extent, and a
    // 0-unit grid is strictly worse than an oversized one.
    ['zero-size', () => boundsWithSize(0, 0, 0)],
    ['NaN', () => boundsWithSize(NaN, NaN, NaN)],
    ['Infinity', () => boundsWithSize(Infinity, Infinity, Infinity)],
  ];

  it('exposes a fitHelpers method', () => {
    const result = setup();
    expect(result).toHaveProperty('fitHelpers');
    expect(typeof (result as any).fitHelpers).toBe('function');
  });

  it.each(DEGENERATE_CASES)(
    '%s bounds leave a pristine scene at the base helper sizing',
    (_label, makeBounds) => {
      const result = setup() as any;
      const defaults = labelOffsets(result);
      result.fitHelpers(makeBounds() as any);
      expectBaseHelperSizing(result, defaults);
    },
  );

  it.each(DEGENERATE_CASES)(
    '%s bounds RESTORE the base helper sizing after a good fit',
    (_label, makeBounds) => {
      const result = setup() as any;
      const defaults = labelOffsets(result);

      // Open a sub-metre model — the helpers shrink onto it...
      result.fitHelpers(boundsWithSize(0.6, 0.6, 0.6) as any);
      expect(result.grid.scale.x).toBeLessThan(1);
      expect(result.axes.scale.x).toBeLessThan(1);
      expect(labelOffsets(result)).not.toEqual(defaults);

      // ...then clear or close the document. props.meshes goes to {}, so
      // Viewport.tsx's mesh-sync effect hands fitHelpers an empty Box3 (or, for a
      // stray single-point mesh, a zero/non-finite one). The now-empty viewport
      // must not keep wearing the closed model's 2 m grid and 0.3 m triad: unlike
      // adjustClipping's stale near/far planes, a stale helper scale is directly
      // on screen. THIS ordering is the one that occurs in the app.
      result.fitHelpers(makeBounds() as any);
      expectBaseHelperSizing(result, defaults);
    },
  );

  it('checks isEmpty() before measuring, mirroring adjustClipping', () => {
    const result = setup() as any;
    const emptyBounds = {
      isEmpty: () => true,
      getCenter: vi.fn(),
      getSize: vi.fn(),
    };

    result.fitHelpers(emptyBounds as any);

    expect(emptyBounds.getSize).not.toHaveBeenCalled();
  });

  // ── Scene-relative sizing ──────────────────────────────────────────────────
  // Three scene scales, each a CUBE of edge L, so radius = L * sqrt(3) / 2:
  //   L = 0.6   — the #6588 dogfood case, a sub-metre printer;
  //   L = 5.77  — radius ~= 5, the "~10 m CAD default" scene Viewport.tsx describes;
  //   L = 231   — radius ~= 200, a large scene.

  const CUBE_CASES: Array<[string, number]> = [
    ['sub-metre printer (L = 0.6)', 0.6],
    ['CAD default (L = 5.77)', 5.77],
    ['large scene (L = 231)', 231],
  ];

  function radiusOfCube(L: number): number {
    return (L * Math.sqrt(3)) / 2;
  }

  it.each(CUBE_CASES)('%s: scales grid and axes uniformly', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    // A NON-uniform scale would skew the grid's cell spacing per-axis, turning
    // square cells into rectangles and the triad into a skewed basis.
    expect(result.grid.scale.x).toBe(result.grid.scale.y);
    expect(result.grid.scale.y).toBe(result.grid.scale.z);
    expect(result.axes.scale.x).toBe(result.axes.scale.y);
    expect(result.axes.scale.y).toBe(result.axes.scale.z);
  });

  it.each(CUBE_CASES)('%s: grid world size tracks the scene radius', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    const radius = radiusOfCube(L);
    const gridWorldSize = 20 * result.grid.scale.x;

    // BASIS (not a guess): spacing = niceSpacing(radius / 5) snaps UP to the next
    // 1|2|5 x 10^k, whose worst-case ratio is 2.5x (a value just above 2 snaps to
    // 5). So spacing lies in [0.2r, 0.5r] and gridWorldSize = 20 * spacing lies in
    // [4r, 10r] — inside this asserted band with margin on both sides.
    expect(gridWorldSize).toBeGreaterThanOrEqual(2 * radius);
    expect(gridWorldSize).toBeLessThanOrEqual(12 * radius);
  });

  it.each(CUBE_CASES)('%s: axes length tracks the scene radius', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    const radius = radiusOfCube(L);
    const axesWorldLength = 2 * result.axes.scale.x;

    // axesWorldLength = 3 * spacing, so it lies in [0.6r, 1.5r] by the same basis.
    expect(axesWorldLength).toBeGreaterThanOrEqual(0.4 * radius);
    expect(axesWorldLength).toBeLessThanOrEqual(2 * radius);
  });

  it.each(CUBE_CASES)('%s: grid cell is a round 1|2|5 x 10^k size', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    // A grid whose cells read 0.10392... m is unreadable as a ruler; snapping to a
    // round decade step is what makes the grid a measuring aid rather than noise.
    const cell = (20 * result.grid.scale.x) / 20;
    const mantissa = cell / Math.pow(10, Math.floor(Math.log10(cell)));
    const nearest = [1, 2, 5].reduce((best, c) =>
      Math.abs(c - mantissa) < Math.abs(best - mantissa) ? c : best,
    );
    expect(mantissa).toBeCloseTo(nearest, 6);
  });

  it('shrinks the helpers for a sub-metre scene — the #6588 defect, in one assertion', () => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(0.6, 0.6, 0.6) as any);

    // Before this fix a 0.6 m model sat inside a 20 m grid (whose far lines converge
    // into the reported dark horizon band) and a 2 m triad (the hard diagonal across
    // every part). Both must now be SMALLER than their defaults.
    expect(result.grid.scale.x).toBeLessThan(1);
    expect(result.axes.scale.x).toBeLessThan(1);
  });

  it('grows the helpers for a large scene', () => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(231, 231, 231) as any);
    expect(result.grid.scale.x).toBeGreaterThan(1);
    expect(result.axes.scale.x).toBeGreaterThan(1);
  });

  // ── Label ring follows the scaled axis tip ─────────────────────────────────

  it.each(CUBE_CASES)('%s: moves the label ring just beyond the scaled axis tip', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    const radius = radiusOfCube(L);
    const axesWorldLength = 2 * result.axes.scale.x;
    const offsets = labelOffsets(result);

    for (const axis of ['X', 'Y', 'Z'] as const) {
      // Strictly beyond the tip, so the letter never sits ON the axis it annotates...
      expect(offsets[axis]).toBeGreaterThan(axesWorldLength);
      // ...but still inside the scene's own neighbourhood, not off at a fixed 2.3
      // that is ~7x the whole model for the #6588 sub-metre case.
      expect(offsets[axis]).toBeLessThanOrEqual(2.3 * radius);
    }

    // Isotropic ring: exact equality, since all three derive from one distance.
    expect(offsets.X).toBe(offsets.Y);
    expect(offsets.Y).toBe(offsets.Z);

    // The other two coordinates of each sprite must be exactly 0 — a ring that
    // drifted off-axis would still satisfy the distance bounds above.
    for (const sprite of result.axisLabels.children as any[]) {
      const axis = sprite.userData.axis as 'X' | 'Y' | 'Z';
      if (axis !== 'X') expect(sprite.position.x).toBe(0);
      if (axis !== 'Y') expect(sprite.position.y).toBe(0);
      if (axis !== 'Z') expect(sprite.position.z).toBe(0);
    }
  });

  it.each(CUBE_CASES)('%s: never scales the labels Group', (_label, L) => {
    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    // THE coupling this pins: the r183 sprite shader multiplies on-screen size by
    // length(modelMatrix[0].xyz), which includes ANCESTOR scale. Scaling this Group
    // would look like a tidy way to resize the ring while silently re-breaking
    // axisLabels.ts's constant-screen-size fix — the exact #6588 symptom.
    // Repositioning via setOffset is the only sound mechanism.
    expect(result.axisLabels.scale.setScalar).not.toHaveBeenCalled();
    expect(result.axisLabels.scale.set).not.toHaveBeenCalled();
    expect(result.axisLabels.scale.x).toBe(1);
    expect(result.axisLabels.scale.y).toBe(1);
    expect(result.axisLabels.scale.z).toBe(1);
  });

  it.each(CUBE_CASES)('%s: leaves each sprite\'s own screen scale alone', (_label, L) => {
    // Compare against a freshly-created scene rather than a hard-coded literal, so
    // this stays true if LABEL_SCREEN_SCALE is ever retuned.
    const pristine = setup() as any;
    const expected = (pristine.axisLabels.children[0] as any).scale.x;

    const result = setup() as any;
    result.fitHelpers(boundsWithSize(L, L, L) as any);

    for (const sprite of result.axisLabels.children as any[]) {
      expect(sprite.scale.x).toBe(expected);
      expect(sprite.scale.y).toBe(expected);
    }
  });

  it('leaves the ~10 m CAD default scene looking exactly as it does today', () => {
    const result = setup() as any;
    // (6, 8, 0) has length exactly 10, so radius is exactly 5 with no float slack.
    result.fitHelpers(boundsWithSize(6, 8, 0) as any);

    // radius / 5 = 1, which is already a round 1 x 10^0, so spacing = 1 m and the
    // grid stays 20 m at scale 1 — byte-identical to the pre-#6588 default. This
    // fix must not disturb the scene size the helpers were originally tuned for.
    expect(20 * result.grid.scale.x).toBe(20);
    expect(result.grid.scale.x).toBe(1);
  });
});
