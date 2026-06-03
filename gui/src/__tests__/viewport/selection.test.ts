import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Track mocks
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();
let mockBox3Instances: any[] = [];

const mockRaycasterSetFromCamera = vi.fn();
const mockRaycasterIntersectObjects = vi.fn((..._args: any[]): any[] => []);

let lastRaycasterInstance: any = null;

vi.mock('three', () => {
  class MockRaycaster {
    setFromCamera = mockRaycasterSetFromCamera;
    intersectObjects = mockRaycasterIntersectObjects;
    firstHitOnly = false;
    constructor() {
      lastRaycasterInstance = this;
    }
  }

  class MockWireframeGeometry {
    dispose = vi.fn();
    constructor(public sourceGeometry?: any) {}
  }

  class MockEdgesGeometry {
    dispose = vi.fn();
    constructor(public sourceGeometry?: any, public thresholdAngle?: number) {}
  }

  class MockLineSegments {
    geometry: any;
    material: any;
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
    }
  }

  class MockLineBasicMaterial {
    color: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
    }
  }

  class MockBox3 {
    min = { x: 0, y: 0, z: 0 };
    max = { x: 1, y: 1, z: 1 };
    expandByObject = vi.fn(() => this);
    getCenter = vi.fn((target: any) => {
      target.x = 0.5;
      target.y = 0.5;
      target.z = 0.5;
      return target;
    });
    getSize = vi.fn((target: any) => {
      target.x = 1;
      target.y = 1;
      target.z = 1;
      return target;
    });
    isEmpty = vi.fn(() => false);
    constructor() {
      mockBox3Instances.push(this);
    }
  }

  class MockVector3 {
    x = 0;
    y = 0;
    z = 0;
    constructor(x?: number, y?: number, z?: number) {
      this.x = x ?? 0;
      this.y = y ?? 0;
      this.z = z ?? 0;
    }
    set(x: number, y: number, z: number) {
      this.x = x;
      this.y = y;
      this.z = z;
      return this;
    }
    copy(v: any) {
      this.x = v.x;
      this.y = v.y;
      this.z = v.z;
      return this;
    }
    sub(v: any) {
      this.x -= v.x;
      this.y -= v.y;
      this.z -= v.z;
      return this;
    }
    multiplyScalar(s: number) {
      this.x *= s;
      this.y *= s;
      this.z *= s;
      return this;
    }
  }

  class MockVector2 {
    x = 0;
    y = 0;
    constructor(x?: number, y?: number) {
      this.x = x ?? 0;
      this.y = y ?? 0;
    }
    set(x: number, y: number) {
      this.x = x;
      this.y = y;
      return this;
    }
  }

  class MockColor {
    value: any;
    constructor(color?: any) {
      this.value = color;
    }
    set(color: any) {
      this.value = color;
      return this;
    }
  }

  class MockMesh {
    geometry: any;
    material: any;
    name: string = '';
    constructor(geometry?: any, material?: any) {
      this.geometry = geometry;
      this.material = material;
    }
  }

  class MockScene {
    add = mockSceneAdd;
    remove = mockSceneRemove;
  }

  class MockMeshStandardMaterial {
    color: any;
    emissive: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color ?? new MockColor(0x000000);
      this.emissive = new MockColor(0x000000);
    }
  }

  class MockPerspectiveCamera {
    fov = 60;
    position = new MockVector3(5, 5, 5);
    lookAt = vi.fn();
    updateProjectionMatrix = vi.fn();
    getWorldDirection = vi.fn((target: any) => {
      // Default: looking along -Z axis (Three.js convention)
      target.x = 0;
      target.y = 0;
      target.z = -1;
      return target;
    });
  }

  class MockBufferGeometry {
    dispose = vi.fn();
  }

  return {
    Raycaster: MockRaycaster,
    WireframeGeometry: MockWireframeGeometry,
    EdgesGeometry: MockEdgesGeometry,
    LineSegments: MockLineSegments,
    LineBasicMaterial: MockLineBasicMaterial,
    Box3: MockBox3,
    Vector3: MockVector3,
    Vector2: MockVector2,
    Color: MockColor,
    Mesh: MockMesh,
    Scene: MockScene,
    MeshStandardMaterial: MockMeshStandardMaterial,
    PerspectiveCamera: MockPerspectiveCamera,
    BufferGeometry: MockBufferGeometry,
  };
});

vi.mock('three-mesh-bvh', () => ({
  acceleratedRaycast: vi.fn(),
  computeBoundsTree: vi.fn(),
  disposeBoundsTree: vi.fn(),
}));

import { createSelection } from '../../viewport/selection';
import { THEME_TOKENS } from '../../theme';
import {
  Scene,
  PerspectiveCamera,
  Mesh,
  MeshStandardMaterial,
  BufferGeometry,
} from 'three';

// rAF mock: synchronous by default so existing tests work unchanged.
// The rAF-throttle describe block overrides with a capturing mock.
let rafCallbacks: Array<FrameRequestCallback> = [];
let rafIdCounter = 1;

function installSyncRaf() {
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    cb(performance.now());
    return rafIdCounter++;
  }) as unknown as typeof requestAnimationFrame;
  globalThis.cancelAnimationFrame = vi.fn() as unknown as typeof cancelAnimationFrame;
}
installSyncRaf();

// Save the synchronous rAF as the "original" for restoring after rAF-throttle tests
const syncRAF = globalThis.requestAnimationFrame;
const syncCAF = globalThis.cancelAnimationFrame;

beforeEach(() => {
  vi.clearAllMocks();
  mockBox3Instances = [];
  rafCallbacks = [];
  rafIdCounter = 1;
  lastRaycasterInstance = null;
});

function createMockDomElement() {
  const el = document.createElement('div');
  // Mock getBoundingClientRect for NDC computation
  el.getBoundingClientRect = () => ({
    left: 0,
    top: 0,
    width: 800,
    height: 600,
    right: 800,
    bottom: 600,
    x: 0,
    y: 0,
    toJSON: () => {},
  });
  return el;
}

function createMockMesh(name: string): any {
  const geom = new BufferGeometry();
  const mat = new MeshStandardMaterial();
  const mesh = new Mesh(geom, mat);
  mesh.name = name;
  return mesh;
}

function createMockControls() {
  const target = { x: 0, y: 0, z: 0, copy: vi.fn((v: any) => { target.x = v.x; target.y = v.y; target.z = v.z; }) };
  return { target };
}

function setup(meshMap?: Map<string, any>, controls?: { target: any }) {
  const scene = new Scene();
  const camera = new PerspectiveCamera();
  const domElement = createMockDomElement();
  const getMeshes = vi.fn(() => meshMap ?? new Map<string, any>());
  const onHover = vi.fn();
  const onSelect = vi.fn();

  const opts: any = {
    scene: scene as any,
    camera: camera as any,
    domElement,
    getMeshes,
    onHover,
    onSelect,
  };
  if (controls !== undefined) {
    opts.controls = controls;
  }

  const selection = createSelection(opts);

  return { scene, camera, domElement, getMeshes, onHover, onSelect, selection };
}

describe('createSelection', () => {
  describe('factory skeleton', () => {
    it('returns an object with setHovered method', () => {
      const { selection } = setup();
      expect(typeof selection.setHovered).toBe('function');
    });

    it('returns an object with setSelected method', () => {
      const { selection } = setup();
      expect(typeof selection.setSelected).toBe('function');
    });

    it('returns an object with fitToView method', () => {
      const { selection } = setup();
      expect(typeof selection.fitToView).toBe('function');
    });

    it('returns an object with dispose method', () => {
      const { selection } = setup();
      expect(typeof selection.dispose).toBe('function');
    });

    it('returns an object with invalidateRect method', () => {
      const { selection } = setup();
      expect(typeof selection.invalidateRect).toBe('function');
    });
  });

  describe('BVH raycasting', () => {
    it('raycaster has firstHitOnly set to true', () => {
      setup();
      expect(lastRaycasterInstance).not.toBeNull();
      expect(lastRaycasterInstance.firstHitOnly).toBe(true);
    });
  });

  describe('hover emissive highlight', () => {
    it('setHovered applies emissive highlight to mesh material', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setHovered('A');

      // emissive should be set to accent color (not black)
      expect(meshA.material.emissive.value).not.toBe(0x000000);
    });

    it('setHovered(null) resets emissive to black', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setHovered('A');
      selection.setHovered(null);

      expect(meshA.material.emissive.value).toBe(0x000000);
    });

    it('changing hover from A to B resets A and highlights B', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      selection.setHovered('A');
      expect(meshA.material.emissive.value).not.toBe(0x000000);

      selection.setHovered('B');
      // A should be reset, B should be highlighted
      expect(meshA.material.emissive.value).toBe(0x000000);
      expect(meshB.material.emissive.value).not.toBe(0x000000);
    });

    it('setHovered with unknown entity path is a no-op', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      // Should not throw
      selection.setHovered('Unknown');
      // A should be unaffected
      expect(meshA.material.emissive.value).toBe(0x000000);
    });
  });

  describe('wireframe overlay on selection', () => {
    it('setSelected creates WireframeGeometry from mesh geometry and adds LineSegments to scene', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');

      // scene.add should have been called with a LineSegments object
      expect(mockSceneAdd).toHaveBeenCalled();
      const addedObj = mockSceneAdd.mock.calls[0][0];
      // The added object should be a LineSegments (has geometry and material)
      expect(addedObj.geometry).toBeDefined();
      expect(addedObj.material).toBeDefined();
    });

    it('setSelected(null) removes LineSegments from scene and disposes wireframe', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframe = mockSceneAdd.mock.calls[0][0];

      selection.setSelected(null);

      expect(mockSceneRemove).toHaveBeenCalledWith(wireframe);
      expect(wireframe.geometry.dispose).toHaveBeenCalled();
    });

    it('changing selection from A to B removes A wireframe and creates B wireframe', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframeA = mockSceneAdd.mock.calls[0][0];

      selection.setSelected('B');

      // A wireframe should be removed
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeA);
      expect(wireframeA.geometry.dispose).toHaveBeenCalled();

      // B wireframe should be added (second call to scene.add)
      expect(mockSceneAdd).toHaveBeenCalledTimes(2);
      const wireframeB = mockSceneAdd.mock.calls[1][0];
      expect(wireframeB.geometry).toBeDefined();
    });

    it('setSelected with unknown entity is a no-op', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      // Should not throw or add anything to scene
      selection.setSelected('Unknown');
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('removeWireframe disposes LineBasicMaterial on deselect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframe = mockSceneAdd.mock.calls[0][0];

      selection.setSelected(null);

      // Material should be disposed alongside geometry
      expect(wireframe.material.dispose).toHaveBeenCalled();
    });

    it('setSelected creates EdgesGeometry (not WireframeGeometry) from mesh geometry', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');

      // The wireframe overlay should use EdgesGeometry
      const addedObj = mockSceneAdd.mock.calls[0][0];
      expect(addedObj.geometry).toBeDefined();
      // EdgesGeometry stores the source geometry
      expect(addedObj.geometry.sourceGeometry).toBe(meshA.geometry);
      // Verify it's an EdgesGeometry instance (has thresholdAngle property)
      expect('thresholdAngle' in addedObj.geometry).toBe(true);
    });

    it('changing selection disposes previous wireframe material', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframeA = mockSceneAdd.mock.calls[0][0];

      selection.setSelected('B');

      // First wireframe's material should be disposed
      expect(wireframeA.material.dispose).toHaveBeenCalled();
    });
  });

  describe('click-based selection raycasting', () => {
    it('calls onSelect with mesh.name on click (pointerdown+pointerup) intersection', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });

    it('calls onSelect with null on click miss', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(onSelect).toHaveBeenCalledWith(null, { ctrl: false, shift: false });
    });

    it('uses raycaster with same NDC computation as hover', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      const ndcArg = mockRaycasterSetFromCamera.mock.calls[0][0];
      expect(ndcArg.x).toBeCloseTo(0, 1);
      expect(ndcArg.y).toBeCloseTo(0, 1);
    });
  });

  describe('fitToView', () => {
    it('computes bounding box and positions camera at appropriate distance', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection, camera } = setup(meshMap);

      selection.fitToView();

      // Box3.expandByObject should be called for each mesh
      // We need to get the Box3 instance - it's created internally
      // Camera should have been repositioned (lookAt called)
      expect((camera as any).lookAt).toHaveBeenCalled();
      expect((camera as any).updateProjectionMatrix).toHaveBeenCalled();
    });

    it('sets camera position based on bounding sphere and FOV', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, camera } = setup(meshMap);

      // Reset camera position to origin so we can verify fitToView moves it
      (camera as any).position.x = 0;
      (camera as any).position.y = 0;
      (camera as any).position.z = 0;

      selection.fitToView();

      // Camera position should be offset from center (0.5, 0.5, 0.5)
      // The distance is computed from bounding box size and FOV
      // With size (1,1,1), maxDim = 1, fov=60, distance = 1 / (2 * tan(30deg)) ≈ 0.866
      // Camera.position.z should be center.z + distance
      const pos = (camera as any).position;
      expect(pos.z).toBeGreaterThan(0.5); // z > center.z
    });

    it('with no meshes, fitToView is a no-op (no crash)', () => {
      const meshMap = new Map<string, any>();
      const { selection, camera } = setup(meshMap);

      // Should not throw
      expect(() => selection.fitToView()).not.toThrow();

      // Camera should not have been repositioned
      expect((camera as any).lookAt).not.toHaveBeenCalled();
    });

    it('calls camera.lookAt with bounding box center', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, camera } = setup(meshMap);

      selection.fitToView();

      // lookAt should be called with center vector (0.5, 0.5, 0.5) from mock
      const lookAtArg = (camera as any).lookAt.mock.calls[0][0];
      expect(lookAtArg.x).toBeCloseTo(0.5);
      expect(lookAtArg.y).toBeCloseTo(0.5);
      expect(lookAtArg.z).toBeCloseTo(0.5);
    });

    it('offsets camera along current view direction, not just Z axis', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, camera } = setup(meshMap);

      // Mock camera looking along -X axis (e.g., viewing from right side)
      (camera as any).getWorldDirection.mockImplementation((target: any) => {
        target.x = -1;
        target.y = 0;
        target.z = 0;
        return target;
      });

      // Reset camera position
      (camera as any).position.x = 0;
      (camera as any).position.y = 0;
      (camera as any).position.z = 0;

      selection.fitToView();

      // Camera should be offset along +X from center (opposite to view direction)
      // center = (0.5, 0.5, 0.5), viewDir = (-1, 0, 0)
      // position = center - viewDir * distance = (0.5 + distance, 0.5, 0.5)
      const pos = (camera as any).position;
      expect(pos.x).toBeGreaterThan(0.5); // offset along X, not Z
      expect(pos.y).toBeCloseTo(0.5);
      expect(pos.z).toBeCloseTo(0.5);
    });

    it('updates controls.target to bounding box center when controls provided', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const controls = createMockControls();
      const { selection } = setup(meshMap, controls);

      selection.fitToView();

      // controls.target should be updated to bounding box center (0.5, 0.5, 0.5)
      expect(controls.target.copy).toHaveBeenCalled();
      const copyArg = controls.target.copy.mock.calls[0][0];
      expect(copyArg.x).toBeCloseTo(0.5);
      expect(copyArg.y).toBeCloseTo(0.5);
      expect(copyArg.z).toBeCloseTo(0.5);
    });

    it('fitToView still works without controls (backward compat)', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      // No controls passed
      const { selection, camera } = setup(meshMap);

      // Should not throw
      expect(() => selection.fitToView()).not.toThrow();
      expect((camera as any).lookAt).toHaveBeenCalled();
    });
  });

  describe('dispose', () => {
    it('removes pointermove event listener from domElement', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, domElement, onHover } = setup(meshMap);

      selection.dispose();

      // After dispose, pointermove should no longer trigger onHover
      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);
      const event = new MouseEvent('pointermove', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(event);

      expect(onHover).not.toHaveBeenCalled();
    });

    it('removes pointer event listeners from domElement', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, domElement, onSelect } = setup(meshMap);

      selection.dispose();

      // After dispose, pointerdown+pointerup should no longer trigger onSelect
      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);
      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300 }));

      expect(onSelect).not.toHaveBeenCalled();
    });

    it('removes existing wireframe from scene on dispose', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      // Create a wireframe by selecting
      selection.setSelected('A');
      const wireframe = mockSceneAdd.mock.calls[0][0];
      mockSceneRemove.mockClear();

      selection.dispose();

      // Wireframe should be removed and geometry disposed
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframe);
      expect(wireframe.geometry.dispose).toHaveBeenCalled();
    });

    it('dispose disposes wireframe material if wireframe exists', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      // Create a wireframe by selecting
      selection.setSelected('A');
      const wireframe = mockSceneAdd.mock.calls[0][0];

      selection.dispose();

      // Material should also be disposed
      expect(wireframe.material.dispose).toHaveBeenCalled();
    });
  });

  describe('flyToEntity', () => {
    it('createSelection returns object with flyToEntity method', () => {
      const { selection } = setup();
      expect(typeof selection.flyToEntity).toBe('function');
    });

    it('flyToEntity positions camera to frame the single entity mesh', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection, camera } = setup(meshMap);

      selection.flyToEntity('A');

      // lookAt should be called with center of mesh A's bounding box
      expect((camera as any).lookAt).toHaveBeenCalled();
      // Camera z should be greater than center z (offset by distance)
      expect((camera as any).position.z).toBeGreaterThan(0.5);
    });

    it('flyToEntity with unknown entity is a no-op', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, camera } = setup(meshMap);

      // Should not throw or call lookAt
      expect(() => selection.flyToEntity('Unknown')).not.toThrow();
      expect((camera as any).lookAt).not.toHaveBeenCalled();
    });

    it('flyToEntity only expands bounding box for the target mesh', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      // Clear instances created during setup (e.g. fitToView's Box3)
      mockBox3Instances = [];

      selection.flyToEntity('A');

      // Get the Box3 instance created inside flyToEntity
      expect(mockBox3Instances.length).toBeGreaterThanOrEqual(1);
      const box3 = mockBox3Instances[mockBox3Instances.length - 1];
      expect(box3.expandByObject).toHaveBeenCalledTimes(1);
      expect(box3.expandByObject).toHaveBeenCalledWith(meshA);
    });

    it('flyToEntity offsets camera along current view direction', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, camera } = setup(meshMap);

      // Mock camera looking along -X axis
      (camera as any).getWorldDirection.mockImplementation((target: any) => {
        target.x = -1;
        target.y = 0;
        target.z = 0;
        return target;
      });

      (camera as any).position.x = 0;
      (camera as any).position.y = 0;
      (camera as any).position.z = 0;

      selection.flyToEntity('A');

      // Camera should be offset along +X from center (opposite to view direction)
      const pos = (camera as any).position;
      expect(pos.x).toBeGreaterThan(0.5);
      expect(pos.y).toBeCloseTo(0.5);
      expect(pos.z).toBeCloseTo(0.5);
    });

    it('flyToEntity updates controls.target to entity bounding box center', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const controls = createMockControls();
      const { selection } = setup(meshMap, controls);

      selection.flyToEntity('A');

      expect(controls.target.copy).toHaveBeenCalled();
      const copyArg = controls.target.copy.mock.calls[0][0];
      expect(copyArg.x).toBeCloseTo(0.5);
      expect(copyArg.y).toBeCloseTo(0.5);
      expect(copyArg.z).toBeCloseTo(0.5);
    });

    it('flyToEntity with unknown entity does not crash or update controls.target', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const controls = createMockControls();
      const { selection } = setup(meshMap, controls);

      expect(() => selection.flyToEntity('Unknown')).not.toThrow();
      expect(controls.target.copy).not.toHaveBeenCalled();
    });
  });

  describe('hover raycasting', () => {
    it('calls raycaster.setFromCamera with NDC coords on pointermove', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      // Simulate pointermove at center of element (400, 300) on 800x600
      const event = new MouseEvent('pointermove', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      // Center of 800x600 → NDC (0, 0)
      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      const ndcArg = mockRaycasterSetFromCamera.mock.calls[0][0];
      expect(ndcArg.x).toBeCloseTo(0, 1);
      expect(ndcArg.y).toBeCloseTo(0, 1);
    });

    it('calls raycaster.intersectObjects with mesh array from getMeshes', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { domElement } = setup(meshMap);

      const event = new MouseEvent('pointermove', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      expect(mockRaycasterIntersectObjects).toHaveBeenCalledTimes(1);
      const meshArray = mockRaycasterIntersectObjects.mock.calls[0][0];
      expect(meshArray).toHaveLength(2);
      expect(meshArray).toContain(meshA);
      expect(meshArray).toContain(meshB);
    });

    it('calls onHover with mesh.name when intersection found', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onHover } = setup(meshMap);

      // Mock raycaster to return an intersection
      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      const event = new MouseEvent('pointermove', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      expect(onHover).toHaveBeenCalledWith('A');
    });

    it('calls onHover with null when no intersection found', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onHover } = setup(meshMap);

      // Default mock returns empty array
      mockRaycasterIntersectObjects.mockReturnValueOnce([]);

      const event = new MouseEvent('pointermove', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      expect(onHover).toHaveBeenCalledWith(null);
    });

    it('computes correct NDC for top-left corner', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      // Top-left: (0, 0) → NDC (-1, 1)
      const event = new MouseEvent('pointermove', {
        clientX: 0,
        clientY: 0,
      });
      domElement.dispatchEvent(event);

      const ndcArg = mockRaycasterSetFromCamera.mock.calls[0][0];
      expect(ndcArg.x).toBeCloseTo(-1, 1);
      expect(ndcArg.y).toBeCloseTo(1, 1);
    });

    it('computes correct NDC for bottom-right corner', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      // Bottom-right: (800, 600) → NDC (1, -1)
      const event = new MouseEvent('pointermove', {
        clientX: 800,
        clientY: 600,
      });
      domElement.dispatchEvent(event);

      const ndcArg = mockRaycasterSetFromCamera.mock.calls[0][0];
      expect(ndcArg.x).toBeCloseTo(1, 1);
      expect(ndcArg.y).toBeCloseTo(-1, 1);
    });
  });

  describe('cached DOMRect', () => {
    it('computeNDC uses cached rect and does not call getBoundingClientRect on every pointermove', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      // Spy on getBoundingClientRect to count calls
      const spy = vi.spyOn(domElement, 'getBoundingClientRect');

      const ev1 = new MouseEvent('pointermove', { clientX: 100, clientY: 100 });
      domElement.dispatchEvent(ev1);

      const ev2 = new MouseEvent('pointermove', { clientX: 200, clientY: 200 });
      domElement.dispatchEvent(ev2);

      // Should have been called only once (lazy population on first event)
      expect(spy).toHaveBeenCalledTimes(1);
    });

    it('invalidateRect causes next pointermove to re-read getBoundingClientRect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, selection } = setup(meshMap);

      const spy = vi.spyOn(domElement, 'getBoundingClientRect');

      // First pointermove populates cache
      const ev1 = new MouseEvent('pointermove', { clientX: 100, clientY: 100 });
      domElement.dispatchEvent(ev1);
      expect(spy).toHaveBeenCalledTimes(1);

      // Invalidate the cache
      selection.invalidateRect();

      // Next pointermove should re-read
      const ev2 = new MouseEvent('pointermove', { clientX: 200, clientY: 200 });
      domElement.dispatchEvent(ev2);
      expect(spy).toHaveBeenCalledTimes(2);
    });

    it('click (pointerdown+pointerup) also uses cached rect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      const spy = vi.spyOn(domElement, 'getBoundingClientRect');

      // First pointermove populates cache
      const ev1 = new MouseEvent('pointermove', { clientX: 100, clientY: 100 });
      domElement.dispatchEvent(ev1);

      // Click should reuse cached rect (raycast happens on pointerup)
      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 200, clientY: 200 }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 200, clientY: 200 }));

      // Still only one call total
      expect(spy).toHaveBeenCalledTimes(1);
    });
  });

  describe('rAF-throttled pointermove', () => {
    function setupWithRaf(meshMap?: Map<string, any>) {
      // Install mock rAF/cAF before creating selection
      globalThis.requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => {
        rafCallbacks.push(cb);
        return rafIdCounter++;
      }) as unknown as typeof requestAnimationFrame;
      globalThis.cancelAnimationFrame = vi.fn((_id: number) => {}) as unknown as typeof cancelAnimationFrame;

      const result = setup(meshMap);
      return result;
    }

    afterEach(() => {
      // Restore synchronous rAF/cAF for other test sections
      globalThis.requestAnimationFrame = syncRAF;
      globalThis.cancelAnimationFrame = syncCAF;
    });

    it('pointermove does not raycast synchronously — stores pending event and schedules rAF', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setupWithRaf(meshMap);

      const ev = new MouseEvent('pointermove', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(ev);

      // Raycaster should NOT have been called synchronously
      expect(mockRaycasterSetFromCamera).not.toHaveBeenCalled();
      // requestAnimationFrame should have been called
      expect(globalThis.requestAnimationFrame).toHaveBeenCalled();
    });

    it('rAF callback performs raycast with latest pending event', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onHover } = setupWithRaf(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      const ev = new MouseEvent('pointermove', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(ev);

      // Invoke the captured rAF callback
      expect(rafCallbacks.length).toBe(1);
      rafCallbacks[0](performance.now());

      // Now raycast should have fired
      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      expect(onHover).toHaveBeenCalledWith('A');
    });

    it('multiple rapid pointermoves only schedule one rAF and raycast uses the last event', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setupWithRaf(meshMap);

      // Dispatch 3 pointermove events with different clientX
      domElement.dispatchEvent(new MouseEvent('pointermove', { clientX: 100, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointermove', { clientX: 200, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointermove', { clientX: 600, clientY: 300 }));

      // Only one rAF should have been scheduled
      expect(globalThis.requestAnimationFrame).toHaveBeenCalledTimes(1);

      // Invoke the callback
      rafCallbacks[0](performance.now());

      // Should have raycasted once, using NDC from last event (clientX=600 on 800-wide → NDC x = 0.5)
      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      const ndcArg = mockRaycasterSetFromCamera.mock.calls[0][0];
      expect(ndcArg.x).toBeCloseTo(0.5, 1);
    });

    it('click (pointerdown+pointerup) raycasts synchronously (not throttled)', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setupWithRaf(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300 }));

      // Click should raycast immediately without rAF
      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });

    it('dispose cancels outstanding rAF', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, selection } = setupWithRaf(meshMap);

      // Dispatch a pointermove to schedule rAF
      domElement.dispatchEvent(new MouseEvent('pointermove', { clientX: 400, clientY: 300 }));
      expect(globalThis.requestAnimationFrame).toHaveBeenCalledTimes(1);
      const rafId = (globalThis.requestAnimationFrame as any).mock.results[0].value;

      selection.dispose();

      // cancelAnimationFrame should have been called with the rAF id
      expect(globalThis.cancelAnimationFrame).toHaveBeenCalledWith(rafId);
    });
  });

  describe('click-vs-drag discrimination', () => {
    it('pointerdown alone does NOT fire onSelect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      // No mockReturnValueOnce needed — pointerdown alone won't trigger raycast

      const event = new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      // onSelect should NOT fire on pointerdown alone — must wait for pointerup
      expect(onSelect).not.toHaveBeenCalled();
    });

    it('pointerdown + pointerup at same position fires onSelect with raycasted entity', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });

    it('pointerdown + pointerup with >5px movement does NOT fire onSelect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      // No mockReturnValueOnce needed — drag won't trigger raycast at all

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      // Move 10px to the right — this is a drag
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 410,
        clientY: 300,
      }));

      expect(onSelect).not.toHaveBeenCalled();
    });

    it('pointerdown + pointerup with <=5px movement fires onSelect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      // Move 3px diagonally (sqrt(9+9)=4.24 < 5) — still a click
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 403,
        clientY: 303,
      }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });

    it('onSelect receives null when pointerup raycast misses', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      // Raycast returns no intersection
      mockRaycasterIntersectObjects.mockReturnValueOnce([]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));
      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(onSelect).toHaveBeenCalledWith(null, { ctrl: false, shift: false });
    });

    it('after dispose, pointerup does not fire onSelect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect, selection } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      // Start a click
      domElement.dispatchEvent(new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      }));

      // Dispose before pointerup
      selection.dispose();

      domElement.dispatchEvent(new MouseEvent('pointerup', {
        clientX: 400,
        clientY: 300,
      }));

      expect(onSelect).not.toHaveBeenCalled();
    });
  });

  describe('ghost exclusion (via meshManager contract)', () => {
    it('raycaster.intersectObjects is called only with meshes from getMeshes, excluding ghosts', () => {
      // Simulate: meshManager.getSceneMeshes() returns only 'show' mesh (meshA)
      // Ghost entity (meshB) is excluded from the map returned by getMeshes
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B'); // ghost entity — excluded from getMeshes
      const meshMap = new Map([['A', meshA]]); // meshB intentionally excluded
      const { domElement } = setup(meshMap);

      const event = new MouseEvent('pointermove', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(event);

      // intersectObjects should only receive meshA, not meshB
      expect(mockRaycasterIntersectObjects).toHaveBeenCalledTimes(1);
      const meshArray = mockRaycasterIntersectObjects.mock.calls[0][0];
      expect(meshArray).toContain(meshA);
      expect(meshArray).not.toContain(meshB);
    });

    it('onHover is not triggered for ghost entity paths excluded from getMeshes', () => {
      // If a ghost entity is excluded from getMeshes, selection can never raycast it,
      // so onHover is never called with that entity path
      const meshA = createMockMesh('A'); // visible 'show' mesh
      // getMeshes only returns meshA (ghost-entity excluded by meshManager)
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onHover } = setup(meshMap);

      // Simulate raycast hit on meshA (the only visible mesh)
      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      const event = new MouseEvent('pointermove', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(event);

      // onHover should be called with 'A', not with 'ghost-entity'
      expect(onHover).toHaveBeenCalledWith('A');
      expect(onHover).not.toHaveBeenCalledWith('ghost-entity');
    });

    it('setSelected does not create wireframe for ghost entity excluded from getMeshes', () => {
      // Ghost entity is not in getMeshes() result
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]); // ghost-entity excluded
      const { selection } = setup(meshMap);

      // Attempt to select a ghost entity (not in getMeshes result)
      selection.setSelected('ghost-entity');

      // No wireframe should be created since mesh not found
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('raycasting is limited to exactly the meshes getMeshes returns', () => {
      // Comprehensive: verify selection system uses exactly what getMeshes returns
      const showMeshA = createMockMesh('show-A');
      const showMeshB = createMockMesh('show-B');
      // Ghost and hidden entities are not in the map
      const meshMap = new Map([['show-A', showMeshA], ['show-B', showMeshB]]);
      const { domElement } = setup(meshMap);

      domElement.dispatchEvent(new MouseEvent('pointermove', { clientX: 400, clientY: 300 }));

      const meshArray = mockRaycasterIntersectObjects.mock.calls[0][0];
      expect(meshArray).toHaveLength(2);
      expect(meshArray).toContain(showMeshA);
      expect(meshArray).toContain(showMeshB);
    });
  });

  describe('multi-wireframe selection', () => {
    it('setSelected([A,B]) creates two wireframes added to scene', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      (selection.setSelected as any)(['A', 'B']);

      // Both wireframes should be added to scene
      expect(mockSceneAdd).toHaveBeenCalledTimes(2);
      // Each wireframe built from the corresponding mesh geometry
      const addedGeometries = mockSceneAdd.mock.calls.map((c: any[]) => c[0].geometry.sourceGeometry);
      expect(addedGeometries).toContain(meshA.geometry);
      expect(addedGeometries).toContain(meshB.geometry);
    });

    it('setSelected([A]) after setSelected([A,B]) removes only B wireframe, preserves A', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      (selection.setSelected as any)(['A', 'B']);

      // Capture the wireframe objects
      const wireframeA = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshA.geometry,
      )?.[0];
      const wireframeB = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshB.geometry,
      )?.[0];
      expect(wireframeA).toBeDefined();
      expect(wireframeB).toBeDefined();

      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      // Shrink selection to just A
      (selection.setSelected as any)(['A']);

      // B wireframe removed and disposed
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeB);
      expect(wireframeB.geometry.dispose).toHaveBeenCalled();
      expect(wireframeB.material.dispose).toHaveBeenCalled();
      // A wireframe NOT removed (still in scene)
      expect(mockSceneRemove).not.toHaveBeenCalledWith(wireframeA);
      // No new wireframe added (A already present)
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('setSelected([]) removes all wireframes', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      (selection.setSelected as any)(['A', 'B']);
      const wireframeA = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshA.geometry,
      )?.[0];
      const wireframeB = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshB.geometry,
      )?.[0];

      mockSceneRemove.mockClear();
      (selection.setSelected as any)([]);

      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeA);
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeB);
      expect(wireframeA.geometry.dispose).toHaveBeenCalled();
      expect(wireframeB.geometry.dispose).toHaveBeenCalled();
    });

    it('setSelected(null) removes all wireframes when multiple are active (backward compat)', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      (selection.setSelected as any)(['A', 'B']);
      const wireframeA = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshA.geometry,
      )?.[0];
      const wireframeB = mockSceneAdd.mock.calls.find(
        (c: any[]) => c[0].geometry.sourceGeometry === meshB.geometry,
      )?.[0];

      mockSceneRemove.mockClear();
      selection.setSelected(null);

      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeA);
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframeB);
    });

    it('setSelected([A,B,C]) where C is unknown only creates wireframes for A and B', () => {
      const meshA = createMockMesh('A');
      const meshB = createMockMesh('B');
      const meshMap = new Map([['A', meshA], ['B', meshB]]);
      const { selection } = setup(meshMap);

      (selection.setSelected as any)(['A', 'B', 'Unknown']);

      // Only A and B found in getMeshes, Unknown skipped
      expect(mockSceneAdd).toHaveBeenCalledTimes(2);
    });
  });

  describe('modifier key routing on click', () => {
    it('plain click calls onSelect with path and { ctrl: false, shift: false }', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300 }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });

    it('Ctrl+click calls onSelect with { ctrl: true, shift: false }', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300, ctrlKey: true }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300, ctrlKey: true }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: true, shift: false });
    });

    it('Shift+click calls onSelect with { ctrl: false, shift: true }', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300, shiftKey: true }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300, shiftKey: true }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: true });
    });

    it('click miss calls onSelect with null and { ctrl: false, shift: false }', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([]);

      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300 }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300 }));

      expect(onSelect).toHaveBeenCalledWith(null, { ctrl: false, shift: false });
    });

    it('modifier state is read from pointerup event (not pointerdown)', () => {
      // A user could theoretically release Ctrl between pointerdown and pointerup;
      // the implementation reads modifiers from the pointerup event.
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      // pointerdown with ctrl, pointerup without ctrl → should see ctrl: false
      domElement.dispatchEvent(new MouseEvent('pointerdown', { clientX: 400, clientY: 300, ctrlKey: true }));
      domElement.dispatchEvent(new MouseEvent('pointerup', { clientX: 400, clientY: 300, ctrlKey: false }));

      expect(onSelect).toHaveBeenCalledWith('A', { ctrl: false, shift: false });
    });
  });

  describe('refreshSelected (V-08)', () => {
    it('exposes refreshSelected method', () => {
      const { selection } = setup();
      expect(typeof selection.refreshSelected).toBe('function');
    });

    it('refreshSelected with active selection removes old wireframe and creates new one', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframe1 = mockSceneAdd.mock.calls[0][0];

      // Clear mocks to track refreshSelected behavior
      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      selection.refreshSelected();

      // Old wireframe should be removed
      expect(mockSceneRemove).toHaveBeenCalledWith(wireframe1);
      expect(wireframe1.geometry.dispose).toHaveBeenCalled();

      // New wireframe should be created
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
      const wireframe2 = mockSceneAdd.mock.calls[0][0];
      expect(wireframe2.geometry).toBeDefined();
      expect(wireframe2.material).toBeDefined();
    });

    it('refreshSelected with no selection is a no-op', () => {
      const { selection } = setup();

      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      selection.refreshSelected();

      expect(mockSceneAdd).not.toHaveBeenCalled();
      expect(mockSceneRemove).not.toHaveBeenCalled();
    });

    it('refreshSelected reflects updated geometry', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');
      const wireframe1 = mockSceneAdd.mock.calls[0][0];
      const origSourceGeom = wireframe1.geometry.sourceGeometry;

      // Simulate geometry update — replace mesh geometry
      const newGeom = new BufferGeometry();
      meshA.geometry = newGeom;

      mockSceneAdd.mockClear();
      selection.refreshSelected();

      // New wireframe should reference the new geometry
      const wireframe2 = mockSceneAdd.mock.calls[0][0];
      expect(wireframe2.geometry.sourceGeometry).toBe(newGeom);
    });
  });

  describe('selection color is high-contrast and distinct from hover', () => {
    it('wireframe material color equals THEME_TOKENS.selection (not accent)', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setup(meshMap);

      selection.setSelected('A');

      const wireframe = mockSceneAdd.mock.calls[0][0];
      expect(wireframe.material.color).toBe(THEME_TOKENS.selection);
    });

    it('THEME_TOKENS.selection is distinct from THEME_TOKENS.accent (hover color)', () => {
      expect(THEME_TOKENS.selection).not.toBe(THEME_TOKENS.accent);
    });
  });

  describe('refreshSelected rAF coalescing', () => {
    function setupWithAsyncRaf(meshMap?: Map<string, any>) {
      // Install async rAF mock before creating selection
      globalThis.requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => {
        rafCallbacks.push(cb);
        return rafIdCounter++;
      }) as unknown as typeof requestAnimationFrame;
      globalThis.cancelAnimationFrame = vi.fn((_id: number) => {}) as unknown as typeof cancelAnimationFrame;

      const result = setup(meshMap);
      return result;
    }

    afterEach(() => {
      // Restore synchronous rAF/cAF for other test sections
      globalThis.requestAnimationFrame = syncRAF;
      globalThis.cancelAnimationFrame = syncCAF;
    });

    it('multiple rapid refreshSelected calls coalesce into one rebuild per frame', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setupWithAsyncRaf(meshMap);

      selection.setSelected('A');

      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      // Call refreshSelected three times in the same frame
      selection.refreshSelected();
      selection.refreshSelected();
      selection.refreshSelected();

      // No rebuild should have happened yet (rAF pending)
      expect(mockSceneRemove).not.toHaveBeenCalled();
      expect(mockSceneAdd).not.toHaveBeenCalled();

      // Only one rAF should have been scheduled (coalescing)
      expect(globalThis.requestAnimationFrame).toHaveBeenCalledTimes(1);

      // Fire the single rAF callback
      rafCallbacks[0](performance.now());

      // Exactly one rebuild: remove + re-add
      expect(mockSceneRemove).toHaveBeenCalledTimes(1);
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
    });

    it('dispose cancels outstanding refreshSelected rAF', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setupWithAsyncRaf(meshMap);

      selection.setSelected('A');

      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      selection.refreshSelected();
      expect(globalThis.requestAnimationFrame).toHaveBeenCalledTimes(1);
      const rafId = (globalThis.requestAnimationFrame as any).mock.results[0].value;

      selection.dispose();

      // cancelAnimationFrame must be called with the refresh rAF id
      expect(globalThis.cancelAnimationFrame).toHaveBeenCalledWith(rafId);

      // If someone were to fire the rAF callback after dispose, nothing should rebuild
      // (isDisposed guard prevents work)
      rafCallbacks[0]?.(performance.now());
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('second refreshSelected call after rAF fires schedules a new rAF', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection } = setupWithAsyncRaf(meshMap);

      selection.setSelected('A');
      mockSceneAdd.mockClear();
      mockSceneRemove.mockClear();

      // First refresh cycle
      selection.refreshSelected();
      rafCallbacks[0](performance.now()); // fire first rAF
      expect(mockSceneRemove).toHaveBeenCalledTimes(1);

      mockSceneRemove.mockClear();
      mockSceneAdd.mockClear();
      rafCallbacks = [];

      // Second refresh cycle after the first completed
      selection.refreshSelected();
      expect(globalThis.requestAnimationFrame).toHaveBeenCalledTimes(2); // total
      rafCallbacks[0](performance.now());
      expect(mockSceneRemove).toHaveBeenCalledTimes(1);
    });
  });
});
