import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { MeshStandardMaterial } from 'three';
import type { MeshData } from '../../types';

// Track all created mocks
const mockGeometries: any[] = [];
const mockMaterials: any[] = [];
const mockMeshes: any[] = [];

const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();
const mockComputeBoundsTree = vi.fn();
const mockDisposeBoundsTree = vi.fn();

vi.mock('three', () => {
  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    index: any = null;
    dispose = vi.fn();
    computeBoundsTree = mockComputeBoundsTree;
    disposeBoundsTree = mockDisposeBoundsTree;
    boundingSphere: any = null;
    boundingBox: any = null;
    constructor() {
      mockGeometries.push(this);
    }

    setAttribute(name: string, attr: any) {
      this.attributes[name] = attr;
    }

    getAttribute(name: string): any {
      return this.attributes[name];
    }

    deleteAttribute(name: string) {
      delete this.attributes[name];
    }

    setIndex(index: any) {
      this.index = index;
    }
  }

  class MockBufferAttribute {
    array: any;
    itemSize: number;
    needsUpdate: boolean = false;
    count: number;
    constructor(array: any, itemSize: number) {
      this.array = array;
      this.itemSize = itemSize;
      this.count = array.length / itemSize;
    }
  }

  class MockMeshStandardMaterial {
    color: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      mockMaterials.push(this);
    }
  }

  class MockMesh {
    geometry: any;
    material: any;
    name: string = '';
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
      mockMeshes.push(this);
    }
  }

  class MockScene {
    add = mockSceneAdd;
    remove = mockSceneRemove;
  }

  class MockColor {
    value: any;
    constructor(color?: any) {
      this.value = color;
    }
  }

  return {
    BufferGeometry: MockBufferGeometry,
    BufferAttribute: MockBufferAttribute,
    MeshStandardMaterial: MockMeshStandardMaterial,
    Mesh: MockMesh,
    Scene: MockScene,
    Color: MockColor,
  };
});

vi.mock('three-mesh-bvh', () => ({
  computeBoundsTree: vi.fn(),
  disposeBoundsTree: vi.fn(),
  acceleratedRaycast: vi.fn(),
}));

import { createMeshManager } from '../../viewport/meshManager';
import { Scene } from 'three';

beforeEach(() => {
  vi.clearAllMocks();
  mockGeometries.length = 0;
  mockMaterials.length = 0;
  mockMeshes.length = 0;
});

function makeMeshData(
  entityPath: string,
  vertices?: Float32Array,
  indices?: Uint32Array,
  normals?: Float32Array | null,
): MeshData {
  return {
    entity_path: entityPath,
    vertices: vertices ?? new Float32Array([0, 1, 2, 3, 4, 5, 6, 7, 8]),
    indices: indices ?? new Uint32Array([0, 1, 2]),
    normals: normals !== undefined ? normals : new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
  };
}

describe('meshManager', () => {
  function setup() {
    const scene = new Scene();
    const manager = createMeshManager(scene);
    return { scene, manager };
  }

  it('returns object with sync, dispose, and getSceneMeshes methods', () => {
    const { manager } = setup();
    expect(typeof manager.sync).toBe('function');
    expect(typeof manager.dispose).toBe('function');
    expect(typeof manager.getSceneMeshes).toBe('function');
  });

  it('sync creates a THREE.Mesh and adds it to scene', () => {
    const { manager } = setup();
    const meshData = makeMeshData('Bracket.body');
    manager.sync({ 'Bracket.body': meshData });

    expect(mockSceneAdd).toHaveBeenCalledTimes(1);
    expect(manager.getSceneMeshes().size).toBe(1);
    expect(manager.getSceneMeshes().has('Bracket.body')).toBe(true);
  });

  it('created mesh geometry has position attribute from vertices', () => {
    const { manager } = setup();
    const verts = new Float32Array([1, 2, 3, 4, 5, 6]);
    const meshData = makeMeshData('A', verts);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.attributes.position).toBeDefined();
    expect(mesh.geometry.attributes.position.array).toBe(verts);
    expect(mesh.geometry.attributes.position.itemSize).toBe(3);
  });

  it('created mesh geometry has index from indices', () => {
    const { manager } = setup();
    const indices = new Uint32Array([0, 1, 2, 2, 3, 0]);
    const meshData = makeMeshData('A', undefined, indices);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.index).toBeDefined();
    expect(mesh.geometry.index!.array).toBe(indices);
  });

  it('created mesh geometry has normal attribute from normals', () => {
    const { manager } = setup();
    const normals = new Float32Array([0, 1, 0, 0, 1, 0]);
    const meshData = makeMeshData('A', undefined, undefined, normals);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.attributes.normal).toBeDefined();
    expect(mesh.geometry.attributes.normal.array).toBe(normals);
  });

  it('sync with updated vertices updates geometry setAttribute', () => {
    const { manager } = setup();
    const meshData1 = makeMeshData('A');
    manager.sync({ A: meshData1 });

    const newVerts = new Float32Array([9, 8, 7, 6, 5, 4]);
    const meshData2 = makeMeshData('A', newVerts);
    manager.sync({ A: meshData2 });

    // Should not have created a new mesh (still 1 in map)
    expect(manager.getSceneMeshes().size).toBe(1);
    // scene.add was called once for initial, no extra add
    expect(mockSceneAdd).toHaveBeenCalledTimes(1);

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.attributes.position.array).toBe(newVerts);
  });

  it('sync with removed entity_path disposes and removes mesh', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A'), B: makeMeshData('B') });
    expect(manager.getSceneMeshes().size).toBe(2);

    const meshA = manager.getSceneMeshes().get('A')!;

    // Remove A by syncing without it
    manager.sync({ B: makeMeshData('B') });

    expect(manager.getSceneMeshes().size).toBe(1);
    expect(manager.getSceneMeshes().has('A')).toBe(false);
    expect(meshA.geometry.dispose).toHaveBeenCalled();
    expect((meshA.material as MeshStandardMaterial).dispose).toHaveBeenCalled();
    expect(mockSceneRemove).toHaveBeenCalledWith(meshA);
  });

  it('each entity_path gets a deterministic color (same path = same color)', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });
    const colorA1 = (manager.getSceneMeshes().get('A')!.material as MeshStandardMaterial).color as any;

    // Recreate and sync again
    manager.sync({});
    manager.sync({ A: makeMeshData('A') });
    const colorA2 = (manager.getSceneMeshes().get('A')!.material as MeshStandardMaterial).color as any;

    // Color for same path should be deterministic — same object value
    expect(colorA2.value).toBe(colorA1.value);
    // djb2 hash of 'A' = charCode 65, abs(65) % 8 = 1, palette[1] = '#cba6f7'
    expect(colorA1.value).toBe('#cba6f7');
  });

  it('different entity paths can get different colors', () => {
    const { manager } = setup();
    manager.sync({
      'Bracket.body': makeMeshData('Bracket.body'),
      'Bracket.hole': makeMeshData('Bracket.hole'),
    });

    const mesh1 = manager.getSceneMeshes().get('Bracket.body')!;
    const mesh2 = manager.getSceneMeshes().get('Bracket.hole')!;

    // Both should have color defined (specific values depend on hash)
    expect((mesh1.material as MeshStandardMaterial).color).toBeDefined();
    expect((mesh2.material as MeshStandardMaterial).color).toBeDefined();
  });

  it('dispose removes and disposes all meshes from scene', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A'), B: makeMeshData('B') });

    const meshA = manager.getSceneMeshes().get('A')!;
    const meshB = manager.getSceneMeshes().get('B')!;

    manager.dispose();

    expect(manager.getSceneMeshes().size).toBe(0);
    expect(meshA.geometry.dispose).toHaveBeenCalled();
    expect((meshA.material as MeshStandardMaterial).dispose).toHaveBeenCalled();
    expect(meshB.geometry.dispose).toHaveBeenCalled();
    expect((meshB.material as MeshStandardMaterial).dispose).toHaveBeenCalled();
    expect(mockSceneRemove).toHaveBeenCalledWith(meshA);
    expect(mockSceneRemove).toHaveBeenCalledWith(meshB);
  });

  it('sync with MeshData where normals is null creates geometry without normal attribute', () => {
    const { manager } = setup();
    const meshData = makeMeshData('A', undefined, undefined, null);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.attributes.position).toBeDefined();
    expect(mesh.geometry.attributes.normal).toBeUndefined();
  });

  it('update reuses existing BufferAttribute objects and sets needsUpdate', () => {
    const { manager } = setup();
    const verts1 = new Float32Array([0, 1, 2, 3, 4, 5]);
    const indices1 = new Uint32Array([0, 1, 2]);
    manager.sync({ A: makeMeshData('A', verts1, indices1) });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    const posAttrBefore = geom.attributes.position;
    const indexBefore = geom.index;

    // Sync with new data
    const verts2 = new Float32Array([9, 8, 7, 6, 5, 4]);
    const indices2 = new Uint32Array([2, 1, 0]);
    manager.sync({ A: makeMeshData('A', verts2, indices2) });

    // Same BufferAttribute object should be reused (identity check)
    expect(geom.attributes.position).toBe(posAttrBefore);
    expect(geom.index).toBe(indexBefore);

    // Data should be updated
    expect(posAttrBefore.array).toBe(verts2);
    expect(indexBefore.array).toBe(indices2);

    // needsUpdate should be flagged
    expect(posAttrBefore.needsUpdate).toBe(true);
    expect(indexBefore.needsUpdate).toBe(true);
  });

  it('update with normals becoming null removes stale normal attribute', () => {
    const { manager } = setup();
    const normals = new Float32Array([0, 0, 1, 0, 0, 1]);
    manager.sync({ A: makeMeshData('A', undefined, undefined, normals) });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    expect(geom.attributes.normal).toBeDefined();

    // Sync the same entity with normals = null
    manager.sync({ A: makeMeshData('A', undefined, undefined, null) });

    // Normal attribute should be removed
    expect(geom.attributes.normal).toBeUndefined();
  });

  describe('R-01: createMeshFromData disposes resources when computeBoundsTree throws', () => {
    it('disposes geometry and material if computeBoundsTree throws', () => {
      const { manager } = setup();
      const meshData = makeMeshData('A');

      // Configure shared mock to throw on the next call (create)
      mockComputeBoundsTree.mockImplementationOnce(() => {
        throw new Error('BVH build failed');
      });

      manager.sync({ A: meshData });

      // Geometry should be disposed (last created geometry)
      const geo = mockGeometries[mockGeometries.length - 1];
      expect(geo.dispose).toHaveBeenCalled();
      // Material should be disposed (last created material)
      const mat = mockMaterials[mockMaterials.length - 1];
      expect(mat.dispose).toHaveBeenCalled();

      // Mesh should NOT be in the map or scene
      expect(manager.getSceneMeshes().has('A')).toBe(false);
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('logs error when computeBoundsTree throws', () => {
      const { manager } = setup();
      const meshData = makeMeshData('A');
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      mockComputeBoundsTree.mockImplementationOnce(() => {
        throw new Error('BVH build failed');
      });

      try {
        manager.sync({ A: meshData });
      } finally {
        consoleSpy.mockRestore();
      }

      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('A'),
        expect.any(Error),
      );
    });
  });

  it('sync with empty incoming object removes all existing meshes', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A'), B: makeMeshData('B') });
    expect(manager.getSceneMeshes().size).toBe(2);

    manager.sync({});

    expect(manager.getSceneMeshes().size).toBe(0);
    expect(mockSceneRemove).toHaveBeenCalledTimes(2);
  });

  it('sync calls geometry.computeBoundsTree() on newly created mesh', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect((mesh.geometry as any).computeBoundsTree).toHaveBeenCalledTimes(1);
  });

  it('sync calls geometry.computeBoundsTree() after updating existing mesh geometry', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });

    const mesh = manager.getSceneMeshes().get('A')!;
    (mesh.geometry as any).computeBoundsTree.mockClear();

    // Update with new vertices
    const newVerts = new Float32Array([9, 8, 7, 6, 5, 4]);
    manager.sync({ A: makeMeshData('A', newVerts) });

    expect((mesh.geometry as any).computeBoundsTree).toHaveBeenCalledTimes(1);
  });

  it('removeMesh calls geometry.disposeBoundsTree() before disposing geometry', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });

    const mesh = manager.getSceneMeshes().get('A')!;

    // Remove A
    manager.sync({});

    expect((mesh.geometry as any).disposeBoundsTree).toHaveBeenCalledTimes(1);
    expect((mesh.geometry as any).dispose).toHaveBeenCalledTimes(1);
  });
});
