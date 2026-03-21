import { describe, it, expect, vi, beforeEach } from 'vitest';

// Track mocks
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

const mockRaycasterSetFromCamera = vi.fn();
const mockRaycasterIntersectObjects = vi.fn((..._args: any[]): any[] => []);

vi.mock('three', () => {
  class MockRaycaster {
    setFromCamera = mockRaycasterSetFromCamera;
    intersectObjects = mockRaycasterIntersectObjects;
  }

  class MockWireframeGeometry {
    dispose = vi.fn();
    constructor(public sourceGeometry?: any) {}
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
  }

  class MockBufferGeometry {
    dispose = vi.fn();
  }

  return {
    Raycaster: MockRaycaster,
    WireframeGeometry: MockWireframeGeometry,
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

import { createSelection } from '../../viewport/selection';
import {
  Scene,
  PerspectiveCamera,
  Mesh,
  MeshStandardMaterial,
  BufferGeometry,
} from 'three';

// rAF mock for throttle tests
let rafCallbacks: Array<FrameRequestCallback> = [];
let rafIdCounter = 1;
const originalRAF = globalThis.requestAnimationFrame;
const originalCAF = globalThis.cancelAnimationFrame;

beforeEach(() => {
  vi.clearAllMocks();
  rafCallbacks = [];
  rafIdCounter = 1;
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

function setup(meshMap?: Map<string, any>) {
  const scene = new Scene();
  const camera = new PerspectiveCamera();
  const domElement = createMockDomElement();
  const getMeshes = vi.fn(() => meshMap ?? new Map<string, any>());
  const onHover = vi.fn();
  const onSelect = vi.fn();

  const selection = createSelection({
    scene: scene as any,
    camera: camera as any,
    domElement,
    getMeshes,
    onHover,
    onSelect,
  });

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
    it('calls onSelect with mesh.name on pointerdown intersection', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      const event = new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      expect(onSelect).toHaveBeenCalledWith('A');
    });

    it('calls onSelect with null on pointerdown miss', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setup(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([]);

      const event = new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

      expect(onSelect).toHaveBeenCalledWith(null);
    });

    it('uses raycaster with same NDC computation as hover', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      const event = new MouseEvent('pointerdown', {
        clientX: 400,
        clientY: 300,
      });
      domElement.dispatchEvent(event);

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

    it('removes pointerdown event listener from domElement', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { selection, domElement, onSelect } = setup(meshMap);

      selection.dispose();

      // After dispose, pointerdown should no longer trigger onSelect
      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);
      const event = new MouseEvent('pointerdown', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(event);

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

    it('pointerdown also uses cached rect', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement } = setup(meshMap);

      const spy = vi.spyOn(domElement, 'getBoundingClientRect');

      // First pointermove populates cache
      const ev1 = new MouseEvent('pointermove', { clientX: 100, clientY: 100 });
      domElement.dispatchEvent(ev1);

      // pointerdown should reuse cached rect
      const ev2 = new MouseEvent('pointerdown', { clientX: 200, clientY: 200 });
      domElement.dispatchEvent(ev2);

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
      // Restore original rAF/cAF
      globalThis.requestAnimationFrame = originalRAF;
      globalThis.cancelAnimationFrame = originalCAF;
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

    it('pointerdown still raycasts synchronously (not throttled)', () => {
      const meshA = createMockMesh('A');
      const meshMap = new Map([['A', meshA]]);
      const { domElement, onSelect } = setupWithRaf(meshMap);

      mockRaycasterIntersectObjects.mockReturnValueOnce([
        { object: meshA, distance: 1, point: { x: 0, y: 0, z: 0 } },
      ]);

      const ev = new MouseEvent('pointerdown', { clientX: 400, clientY: 300 });
      domElement.dispatchEvent(ev);

      // Pointerdown should raycast immediately without rAF
      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledWith('A');
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
});
