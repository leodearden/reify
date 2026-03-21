import { describe, it, expect, vi, beforeEach } from 'vitest';

// Track mocks
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

const mockRaycasterSetFromCamera = vi.fn();
const mockRaycasterIntersectObjects = vi.fn(() => []);

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

beforeEach(() => {
  vi.clearAllMocks();
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
});
