import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { MeshStandardMaterial } from 'three';
import type { MeshData, RawMeshData } from '../../types';

// Track all created mocks.
// mockBasicMaterials and mockPhongMaterials use vi.hoisted so they are initialized
// before the async vi.mock factory runs (async factories run before module-level
// const declarations). The arguments are evaluated eagerly at factory-execution time,
// so the arrays must already exist at that point.
const mockBasicMaterials = vi.hoisted<any[]>(() => []);
const mockPhongMaterials = vi.hoisted<any[]>(() => []);
// These arrays do NOT need vi.hoisted: each is only referenced by a class
// constructor closure (captured by reference, dereferenced at construction time
// rather than during factory execution), so plain const declarations are fine.
const mockGeometries: any[] = [];
const mockMaterials: any[] = [];
const mockMeshes: any[] = [];
const mockGroups: any[] = [];

const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();
const mockComputeBoundsTree = vi.fn();
const mockDisposeBoundsTree = vi.fn();
const mockGroupAdd = vi.fn();
const mockGroupRemove = vi.fn();

vi.mock('three', async () => {
  const { makeMockMeshBasicMaterial, makeMockMeshPhongMaterial } = await import('./mocks/threeMocks');
  const MockMeshBasicMaterial = makeMockMeshBasicMaterial(mockBasicMaterials);
  const MockMeshPhongMaterial = makeMockMeshPhongMaterial(mockPhongMaterials);

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

    computeVertexNormals = vi.fn();

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
    dispose = vi.fn();
    constructor(array: any, itemSize: number) {
      this.array = array;
      this.itemSize = itemSize;
      this.count = array.length / itemSize;
    }
    clone() {
      return new MockBufferAttribute(this.array.slice(), this.itemSize);
    }
  }

  class MockMeshStandardMaterial {
    color: any;
    side: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      this.side = opts?.side;
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

  class MockGroup {
    add = mockGroupAdd;
    remove = mockGroupRemove;
    name: string = '';
    constructor() {
      mockGroups.push(this);
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
    MeshPhongMaterial: MockMeshPhongMaterial,
    MeshBasicMaterial: MockMeshBasicMaterial,
    Mesh: MockMesh,
    Group: MockGroup,
    Scene: MockScene,
    Color: MockColor,
    DoubleSide: 2,
    FrontSide: 0,
  };
});

vi.mock('three-mesh-bvh', () => ({
  computeBoundsTree: vi.fn(),
  disposeBoundsTree: vi.fn(),
  acceleratedRaycast: vi.fn(),
}));

import { createMeshManager } from '../../viewport/meshManager';
import { convertRawMesh } from '../../types';
import { Scene } from 'three';

beforeEach(() => {
  vi.clearAllMocks();
  mockGeometries.length = 0;
  mockMaterials.length = 0;
  mockMeshes.length = 0;
  mockBasicMaterials.length = 0;
  mockPhongMaterials.length = 0;
  mockGroups.length = 0;
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
    // createMeshManager adds ghostGroup to scene — clear mock history so tests
    // don't see that spurious scene.add call in their counts.
    vi.clearAllMocks();
    return { scene, manager };
  }

  function setupDeformableMesh(extraMeshes: Record<string, MeshData> = {}) {
    const scene = new Scene();
    const manager = createMeshManager(scene);
    vi.clearAllMocks();
    const vertices = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const displaced = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);
    const meshA: MeshData = {
      entity_path: 'A',
      vertices: vertices.slice(),
      indices: new Uint32Array([0, 1, 2]),
      normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
      displaced_positions: displaced.slice(),
    };
    manager.sync({ A: meshA, ...extraMeshes });
    vi.clearAllMocks();
    return { scene, manager, vertices, displaced };
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
    const verts = new Float32Array([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    const meshData = makeMeshData('A', verts);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    expect(mesh.geometry.attributes.position).toBeDefined();
    // Copy semantics: the position buffer must NOT alias the caller's array …
    expect(mesh.geometry.attributes.position.array).not.toBe(verts);
    // … but must contain identical values.
    expect(Array.from(mesh.geometry.attributes.position.array as Float32Array)).toEqual(Array.from(verts));
    expect(mesh.geometry.attributes.position.itemSize).toBe(3);
  });

  it('created mesh geometry has index from indices', () => {
    const { manager } = setup();
    // 4 vertices via default (9 floats = 3 vertices) — use custom verts for 4
    const verts = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0]);
    const indices = new Uint32Array([0, 1, 2, 2, 3, 0]);
    const meshData = makeMeshData('A', verts, indices);
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

    const newVerts = new Float32Array([9, 8, 7, 6, 5, 4, 3, 2, 1]);
    const meshData2 = makeMeshData('A', newVerts);
    manager.sync({ A: meshData2 });

    // Should not have created a new mesh (still 1 in map)
    expect(manager.getSceneMeshes().size).toBe(1);
    // scene.add was called once for initial, no extra add
    expect(mockSceneAdd).toHaveBeenCalledTimes(1);

    const mesh = manager.getSceneMeshes().get('A')!;
    // Copy semantics: position buffer must NOT alias the caller's array after re-sync …
    expect(mesh.geometry.attributes.position.array).not.toBe(newVerts);
    // … but must contain identical values.
    expect(Array.from(mesh.geometry.attributes.position.array as Float32Array)).toEqual(Array.from(newVerts));
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
    const verts1 = new Float32Array([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    const indices1 = new Uint32Array([0, 1, 2]);
    manager.sync({ A: makeMeshData('A', verts1, indices1) });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    const posAttrBefore = geom.attributes.position;
    const indexBefore = geom.index;

    // Sync with new data (same length)
    const verts2 = new Float32Array([9, 8, 7, 6, 5, 4, 3, 2, 1]);
    const indices2 = new Uint32Array([2, 1, 0]);
    manager.sync({ A: makeMeshData('A', verts2, indices2) });

    // Same BufferAttribute object should be reused (identity check)
    expect(geom.attributes.position).toBe(posAttrBefore);
    expect(geom.index).toBe(indexBefore);

    // Data should be updated — position uses copy semantics (not same reference),
    // but index array is directly aliased (indices are never mutated in-place).
    expect(posAttrBefore.array).not.toBe(verts2);
    expect(Array.from(posAttrBefore.array as Float32Array)).toEqual(Array.from(verts2));
    expect(indexBefore.array).toBe(indices2);

    // needsUpdate should be flagged
    expect(posAttrBefore.needsUpdate).toBe(true);
    expect(indexBefore.needsUpdate).toBe(true);
  });

  it('createMeshFromData calls computeVertexNormals when normals is null (V-04)', () => {
    const { manager } = setup();
    const meshData = makeMeshData('A', undefined, undefined, null);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    expect(geom.computeVertexNormals).toHaveBeenCalled();
  });

  it('createMeshFromData does NOT call computeVertexNormals when normals are provided (V-04)', () => {
    const { manager } = setup();
    const normals = new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]);
    const meshData = makeMeshData('A', undefined, undefined, normals);
    manager.sync({ A: meshData });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    expect(geom.computeVertexNormals).not.toHaveBeenCalled();
  });

  it('material is created with side: DoubleSide (V-05)', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });

    const mesh = manager.getSceneMeshes().get('A')!;
    const material = mesh.material as any;
    // THREE.DoubleSide === 2
    expect(material.side).toBe(2);
  });

  it('updateMeshGeometry calls computeVertexNormals when normals become null (V-04)', () => {
    const { manager } = setup();
    const normals = new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]);
    manager.sync({ A: makeMeshData('A', undefined, undefined, normals) });

    const mesh = manager.getSceneMeshes().get('A')!;
    const geom = mesh.geometry as any;
    geom.computeVertexNormals.mockClear();

    // Update with null normals
    manager.sync({ A: makeMeshData('A', undefined, undefined, null) });
    expect(geom.computeVertexNormals).toHaveBeenCalled();
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

  describe('safe buffer updates on array length change (V-07)', () => {
    it('update with same-length arrays reuses existing BufferAttribute', () => {
      const { manager } = setup();
      const verts1 = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      const indices1 = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts1, indices1) });

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const posAttrBefore = geom.attributes.position;
      const indexBefore = geom.index;

      // Same length arrays
      const verts2 = new Float32Array([9, 9, 9, 8, 8, 8, 7, 7, 7]);
      const indices2 = new Uint32Array([2, 1, 0]);
      manager.sync({ A: makeMeshData('A', verts2, indices2) });

      // Same BufferAttribute reference (reused)
      expect(geom.attributes.position).toBe(posAttrBefore);
      expect(geom.index).toBe(indexBefore);
    });

    it('update with different-length vertex array creates new BufferAttribute', () => {
      const { manager } = setup();
      const verts1 = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      const indices1 = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts1, indices1) });

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const posAttrBefore = geom.attributes.position;

      // Different length — 4 vertices instead of 3
      const verts2 = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3]);
      const indices2 = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts2, indices2) });

      // Should be a NEW BufferAttribute with copy semantics.
      expect(geom.attributes.position).not.toBe(posAttrBefore);
      expect(geom.attributes.position.array).not.toBe(verts2);
      expect(Array.from(geom.attributes.position.array as Float32Array)).toEqual(Array.from(verts2));
    });

    it('update with different-length index array creates new index BufferAttribute', () => {
      const { manager } = setup();
      const verts = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3]);
      const indices1 = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts, indices1) });

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const indexBefore = geom.index;

      // Different length — 6 indices instead of 3
      const indices2 = new Uint32Array([0, 1, 2, 2, 3, 0]);
      manager.sync({ A: makeMeshData('A', verts, indices2) });

      // Should be a NEW index BufferAttribute
      expect(geom.index).not.toBe(indexBefore);
      expect(geom.index.array).toBe(indices2);
    });

    it('new BufferAttribute has correct array and count', () => {
      const { manager } = setup();
      const verts1 = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      manager.sync({ A: makeMeshData('A', verts1) });

      // Update with 4 vertices
      const verts2 = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3]);
      const indices2 = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts2, indices2) });

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      // Copy semantics: the position array must be a copy, not the caller's buffer.
      expect(geom.attributes.position.array).not.toBe(verts2);
      expect(Array.from(geom.attributes.position.array as Float32Array)).toEqual(Array.from(verts2));
      expect(geom.attributes.position.count).toBe(4); // 12 / 3
      expect(geom.attributes.position.itemSize).toBe(3);
    });
  });

  describe('mesh data validation (V-06)', () => {
    it('sync with vertices.length not divisible by 3 does not add mesh to scene', () => {
      const { manager } = setup();
      // 5 floats is not divisible by 3
      const badVerts = new Float32Array([0, 1, 2, 3, 4]);
      const meshData = makeMeshData('A', badVerts, new Uint32Array([0]));
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      manager.sync({ A: meshData });
      expect(manager.getSceneMeshes().size).toBe(0);
      expect(mockSceneAdd).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it('sync with an index >= vertex count does not add mesh to scene', () => {
      const { manager } = setup();
      // 3 vertices (indices 0, 1, 2), but index references vertex 3 (out of bounds)
      const verts = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      const indices = new Uint32Array([0, 1, 3]); // 3 is out of bounds
      const meshData = makeMeshData('A', verts, indices);
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      manager.sync({ A: meshData });
      expect(manager.getSceneMeshes().size).toBe(0);
      expect(mockSceneAdd).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it('valid data still works normally after validation', () => {
      const { manager } = setup();
      // 3 vertices, valid indices
      const verts = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      const indices = new Uint32Array([0, 1, 2]);
      const meshData = makeMeshData('A', verts, indices);
      manager.sync({ A: meshData });
      expect(manager.getSceneMeshes().size).toBe(1);
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
    });

    it('update with invalid data skips the update', () => {
      const { manager } = setup();
      // Valid initial data
      const verts = new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
      const indices = new Uint32Array([0, 1, 2]);
      manager.sync({ A: makeMeshData('A', verts, indices) });
      expect(manager.getSceneMeshes().size).toBe(1);

      // Update with invalid vertices
      const badVerts = new Float32Array([0, 1]); // not divisible by 3
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      manager.sync({ A: makeMeshData('A', badVerts, indices) });

      // Mesh should still exist but geometry should not have been updated with bad data
      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      // Position should still have the original data (copy semantics: not the same reference,
      // but must contain identical values since the bad-data update was rejected).
      expect(Array.from(geom.attributes.position.array as Float32Array)).toEqual(Array.from(verts));
      warnSpy.mockRestore();
    });
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

        expect(consoleSpy).toHaveBeenCalledWith(
          expect.stringContaining('A'),
          expect.any(Error),
        );
      } finally {
        consoleSpy.mockRestore();
      }
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

    // Update with new vertices (must have ≥3 vertices to pass V-06 validation with default indices [0,1,2])
    const newVerts = new Float32Array([9, 8, 7, 6, 5, 4, 3, 2, 1]);
    manager.sync({ A: makeMeshData('A', newVerts) });

    expect((mesh.geometry as any).computeBoundsTree).toHaveBeenCalledTimes(1);
  });

  describe('R-02: updateMeshGeometry removes mesh when computeBoundsTree throws on update', () => {
    it('removes mesh from scene and meshMap if computeBoundsTree throws on update', () => {
      const { manager } = setup();
      manager.sync({ A: makeMeshData('A') });

      const mesh = manager.getSceneMeshes().get('A')!;
      expect(manager.getSceneMeshes().has('A')).toBe(true);

      // Configure next computeBoundsTree call (the update) to throw
      mockComputeBoundsTree.mockImplementationOnce(() => {
        throw new Error('BVH rebuild failed');
      });

      // Update with new vertices — computeBoundsTree will throw
      // Must have ≥3 vertices to pass V-06 validation with default indices [0,1,2]
      const newVerts = new Float32Array([9, 8, 7, 6, 5, 4, 3, 2, 1]);
      manager.sync({ A: makeMeshData('A', newVerts) });

      // Mesh should be fully removed
      expect(manager.getSceneMeshes().has('A')).toBe(false);
      expect(mockSceneRemove).toHaveBeenCalledWith(mesh);
      expect(mesh.geometry.dispose).toHaveBeenCalled();
      expect((mesh.material as MeshStandardMaterial).dispose).toHaveBeenCalled();
    });

    it('logs error when computeBoundsTree throws on update', () => {
      const { manager } = setup();
      manager.sync({ A: makeMeshData('A') });

      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      mockComputeBoundsTree.mockImplementationOnce(() => {
        throw new Error('BVH rebuild failed');
      });

      try {
        // Must have ≥3 vertices to pass V-06 validation with default indices [0,1,2]
        const newVerts = new Float32Array([9, 8, 7, 6, 5, 4, 3, 2, 1]);
        manager.sync({ A: makeMeshData('A', newVerts) });

        expect(consoleSpy).toHaveBeenCalledWith(
          expect.stringContaining('A'),
          expect.any(Error),
        );
      } finally {
        consoleSpy.mockRestore();
      }
    });
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

  describe('scalar_channels backwards-compat (C-01)', () => {
    it('createMeshManager without colorize option: mesh with scalar_channels still uses MeshStandardMaterial', () => {
      const { manager } = setup();
      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };

      manager.sync({ A: meshData });

      const mesh = manager.getSceneMeshes().get('A')!;
      expect(mesh).toBeDefined();

      // Material should be in mockMaterials (MeshStandardMaterial), not a phong material
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);

      // Geometry must have NO 'color' attribute
      expect((mesh.geometry as any).attributes.color).toBeUndefined();
    });

    it('createMeshManager without colorize option: mesh with multiple scalar_channels has no color attribute', () => {
      const { manager } = setup();
      const meshData: MeshData = {
        entity_path: 'B',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
        scalar_channels: {
          vonMises: new Float32Array([10, 20, 30]),
          displacement_magnitude: new Float32Array([0.1, 0.2, 0.3]),
        },
      };

      const warnSpy = vi.spyOn(console, 'warn');
      manager.sync({ B: meshData });
      expect(warnSpy).not.toHaveBeenCalled();
      warnSpy.mockRestore();

      const mesh = manager.getSceneMeshes().get('B')!;
      expect(mesh).toBeDefined();

      // No color attribute — scalar_channels are ignored when colorize is unset
      expect((mesh.geometry as any).attributes.color).toBeUndefined();
    });
  });

  describe('colorize path (C-02)', () => {
    const sentinelBake = (s: Float32Array) =>
      new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);

    it('headline: mesh with matching scalar_channel uses MeshPhongMaterial with vertexColors', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
      manager.sync({ A: meshData });

      const mesh = manager.getSceneMeshes().get('A')!;
      expect(mesh).toBeDefined();

      // (a) material is a phong material, not a standard material
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(false);

      // (b) phong material options
      const mat = mesh.material as any;
      expect(mat.vertexColors).toBe(true);
      expect(mat.flatShading).toBe(false);
      expect(mat.side).toBe(2); // DoubleSide

      // (c) geometry has color attribute with baked scalars
      const colorAttr = (mesh.geometry as any).attributes.color;
      expect(colorAttr).toBeDefined();
      expect(colorAttr.itemSize).toBe(3);
      expect(Array.from(colorAttr.array as Float32Array)).toEqual([10, 0, 0, 20, 0, 0, 30, 0, 0]);
    });

    it('channel-presence gate: mesh missing the colorize channel falls back to MeshStandardMaterial', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      // Only has displacement_magnitude, NOT vonMises
      const meshData: MeshData = {
        entity_path: 'B',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { displacement_magnitude: new Float32Array([0.1, 0.2, 0.3]) },
      };
      manager.sync({ B: meshData });

      const mesh = manager.getSceneMeshes().get('B')!;
      expect(mesh).toBeDefined();

      // Falls back to standard material — channel not present
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(false);

      // No color attribute
      expect((mesh.geometry as any).attributes.color).toBeUndefined();
    });

    it('mesh with no scalar_channels at all falls back to MeshStandardMaterial', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'C',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        // no scalar_channels field
      };
      manager.sync({ C: meshData });

      const mesh = manager.getSceneMeshes().get('C')!;
      expect(mesh).toBeDefined();

      // Falls back to standard material — no scalar_channels
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(false);
      expect((mesh.geometry as any).attributes.color).toBeUndefined();
    });
  });

  describe('setColorize in-place mutation (C-03)', () => {
    const redBake = (s: Float32Array) =>
      new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);
    const greenBake = (s: Float32Array) =>
      new Float32Array([0, s[0], 0, 0, s[1], 0, 0, s[2], 0]);

    function setupColorized() {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: redBake },
      });
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
      manager.sync({ A: meshData });
      vi.clearAllMocks();

      return { scene, manager };
    }

    it('(a) setColorize reuses the same color BufferAttribute reference', () => {
      const { manager } = setupColorized();

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const savedRef = geom.attributes.color;
      expect(savedRef).toBeDefined();

      manager.setColorize({ channel: 'vonMises', bake: greenBake });

      // Same BufferAttribute object (identity check)
      expect(geom.attributes.color).toBe(savedRef);
    });

    it('(b) setColorize updates the color array to the new bake output', () => {
      const { manager } = setupColorized();

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;

      manager.setColorize({ channel: 'vonMises', bake: greenBake });

      const colorAttr = geom.attributes.color;
      // greenBake([10,20,30]) = [0,10,0, 0,20,0, 0,30,0]
      expect(Array.from(colorAttr.array as Float32Array)).toEqual([0, 10, 0, 0, 20, 0, 0, 30, 0]);
    });

    it('(c) setColorize sets needsUpdate = true on the color attribute', () => {
      const { manager } = setupColorized();

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const colorAttr = geom.attributes.color;
      colorAttr.needsUpdate = false; // explicitly reset

      manager.setColorize({ channel: 'vonMises', bake: greenBake });

      expect(colorAttr.needsUpdate).toBe(true);
    });

    it('(d) setColorize does NOT create new geometry, meshes, or materials', () => {
      const { manager } = setupColorized();

      const geomCountBefore = mockGeometries.length;
      const meshCountBefore = mockMeshes.length;
      const phongCountBefore = mockPhongMaterials.length;

      manager.setColorize({ channel: 'vonMises', bake: greenBake });

      expect(mockGeometries.length).toBe(geomCountBefore);
      expect(mockMeshes.length).toBe(meshCountBefore);
      expect(mockPhongMaterials.length).toBe(phongCountBefore);
    });

    it('(e) setColorize with a different channel name re-bakes from the new channel scalars', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: redBake },
      });
      vi.clearAllMocks();

      // Mesh exposes BOTH channels
      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: {
          vonMises: new Float32Array([10, 20, 30]),
          displacement_magnitude: new Float32Array([1, 2, 3]),
        },
      };
      manager.sync({ A: meshData });
      vi.clearAllMocks();

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const savedRef = geom.attributes.color;

      // Switch to the other channel; bake function for the new channel:
      // greenBake([1, 2, 3]) = [0, 1, 0, 0, 2, 0, 0, 3, 0]
      manager.setColorize({ channel: 'displacement_magnitude', bake: greenBake });

      // Same buffer reference (in-place)
      expect(geom.attributes.color).toBe(savedRef);
      // Array rebaked from displacement_magnitude scalars
      expect(Array.from(geom.attributes.color.array as Float32Array)).toEqual([
        0, 1, 0, 0, 2, 0, 0, 3, 0,
      ]);
    });

    it('(f) sync→sync: updateMeshGeometry refreshes scalar channels and re-bakes colour buffer', () => {
      // This test guards suggestion-1: updateMeshGeometry must update meshScalarChannels
      // and re-bake the colour attribute in place when colorize is active.
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: redBake },
      });
      vi.clearAllMocks();

      // First sync: 3 vertices with vonMises [10, 20, 30]
      const meshData1: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
      manager.sync({ A: meshData1 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const savedColorRef = geom.attributes.color;
      expect(savedColorRef).toBeDefined();
      // redBake([10, 20, 30]) = [10, 0, 0, 20, 0, 0, 30, 0, 0]
      expect(Array.from(savedColorRef.array as Float32Array)).toEqual([10, 0, 0, 20, 0, 0, 30, 0, 0]);

      vi.clearAllMocks();

      // Second sync: same entity, updated vonMises [40, 50, 60]
      const meshData2: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([40, 50, 60]) },
      };
      manager.sync({ A: meshData2 });

      // Same buffer reference (in-place mutation, not a new attribute)
      expect(geom.attributes.color).toBe(savedColorRef);
      // Colour re-baked from updated vonMises: redBake([40, 50, 60]) = [40, 0, 0, 50, 0, 0, 60, 0, 0]
      expect(Array.from(geom.attributes.color.array as Float32Array)).toEqual(
        [40, 0, 0, 50, 0, 0, 60, 0, 0],
      );
      expect(geom.attributes.color.needsUpdate).toBe(true);
      // No new geometry, mesh, or material created — update path only
      expect(mockGeometries.length).toBe(1);
      expect(mockMeshes.length).toBe(1);
    });

    it('(g) setColorize(null) clears colorize; subsequent setColorize re-bakes existing colour buffer', () => {
      // Tests the asymmetric behaviour: meshes retain MeshPhongMaterial after
      // setColorize(null) but their colour buffer can be re-baked by a subsequent call.
      const { manager } = setupColorized();

      const mesh = manager.getSceneMeshes().get('A')!;
      const geom = mesh.geometry as any;
      const savedRef = geom.attributes.color;

      // Toggle off — colorize state cleared
      manager.setColorize(null);

      // Toggle back on with a blue-ramp bake
      const blueBake = (s: Float32Array) =>
        new Float32Array([0, 0, s[0], 0, 0, s[1], 0, 0, s[2]]);

      manager.setColorize({ channel: 'vonMises', bake: blueBake });

      // Same buffer reference — material was chosen at creation time, not replaced
      expect(geom.attributes.color).toBe(savedRef);
      // blueBake([10, 20, 30]) = [0, 0, 10, 0, 0, 20, 0, 0, 30]
      expect(Array.from(geom.attributes.color.array as Float32Array)).toEqual(
        [0, 0, 10, 0, 0, 20, 0, 0, 30],
      );
      expect(geom.attributes.color.needsUpdate).toBe(true);
      // No new geometry or materials — in-place update only
      expect(mockPhongMaterials.length).toBe(1);
    });
  });

  describe('colorize material disposal (C-04)', () => {
    const sentinelBake = (s: Float32Array) =>
      new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);

    function makeColorizedMeshData(entityPath: string): MeshData {
      return {
        entity_path: entityPath,
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
    }

    it('(a) dispose() on a manager with a colorized mesh calls dispose() on the MeshPhongMaterial', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      manager.sync({ A: makeColorizedMeshData('A') });

      const mesh = manager.getSceneMeshes().get('A')!;
      const mat = mesh.material as any;

      // Verify the material is a phong material
      expect(mockPhongMaterials.some((m: any) => m === mat)).toBe(true);
      expect(mat.dispose).not.toHaveBeenCalled();

      manager.dispose();

      // Material dispose must have been called (no resource leak)
      expect(mat.dispose).toHaveBeenCalled();
    });

    it('(b) sync() removing a colorized mesh disposes its MeshPhongMaterial', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      manager.sync({ A: makeColorizedMeshData('A'), B: makeColorizedMeshData('B') });

      const meshA = manager.getSceneMeshes().get('A')!;
      const matA = meshA.material as any;
      expect(mockPhongMaterials.some((m: any) => m === matA)).toBe(true);

      // Remove A by syncing without it
      manager.sync({ B: makeColorizedMeshData('B') });

      expect(matA.dispose).toHaveBeenCalled();
    });

    it('(c) MeshPhongMaterial disposal does not affect the remaining mesh', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      manager.sync({ A: makeColorizedMeshData('A'), B: makeColorizedMeshData('B') });

      const meshB = manager.getSceneMeshes().get('B')!;
      const matB = meshB.material as any;

      // Remove A
      manager.sync({ B: makeColorizedMeshData('B') });

      // B's material should NOT have been disposed
      expect(matB.dispose).not.toHaveBeenCalled();
      // B is still in the scene
      expect(manager.getSceneMeshes().has('B')).toBe(true);
    });
  });

  describe('ghost visibility', () => {
    // Helper: create manager, add one mesh, then reset mock call history so tests
    // start with zero recorded call counts.
    //
    // State cleared by vi.clearAllMocks():
    //   - Call counts and arguments for all vi.fn() mocks (scene.add, scene.remove, Group.add, etc.)
    //   - Spy call counts, including .dispose spies on individual MeshBasicMaterial instances
    //     (so tests that check .dispose after setupWithMesh must know counts were reset to 0).
    //
    // State NOT cleared by vi.clearAllMocks():
    //   - mockBasicMaterials array (purposefully preserved so test (c) can find the ghost
    //     material instance created during createMeshManager, which precedes the clearAllMocks calls).
    //   - mockGroups array (likewise preserved for Group identity checks).
    function setupWithMesh(entityPath = 'A') {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      // Manager creation adds ghostGroup to scene — clear call history.
      vi.clearAllMocks();
      manager.sync({ [entityPath]: makeMeshData(entityPath) });
      // Clear again so tests start with zero recorded add/remove calls.
      vi.clearAllMocks();
      return { scene, manager };
    }

    it('setVisibility and getGhostMeshes methods exist on manager', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      expect(typeof (manager as any).setVisibility).toBe('function');
      expect(typeof (manager as any).getGhostMeshes).toBe('function');
    });

    it('(a) setVisibility ghost removes mesh from scene and adds ghost clone to ghostGroup', () => {
      const { manager } = setupWithMesh();
      const mesh = manager.getSceneMeshes().get('A')!;

      (manager as any).setVisibility('A', 'ghost');

      expect(mockSceneRemove).toHaveBeenCalledWith(mesh);
      expect(mockGroupAdd).toHaveBeenCalledTimes(1);
    });

    it('(b) ghost clone shares geometry reference with original mesh', () => {
      const { manager } = setupWithMesh();
      const originalMesh = manager.getSceneMeshes().get('A')!;
      const originalGeometry = originalMesh.geometry;

      (manager as any).setVisibility('A', 'ghost');

      const ghostMap: Map<string, any> = (manager as any).getGhostMeshes();
      const ghostMesh = ghostMap.get('A')!;
      expect(ghostMesh).toBeDefined();
      expect(ghostMesh.geometry).toBe(originalGeometry); // identity check
    });

    it('(c) ghost clone uses MeshBasicMaterial, not MeshStandardMaterial', () => {
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');

      const ghostMap: Map<string, any> = (manager as any).getGhostMeshes();
      const ghostMesh = ghostMap.get('A')!;
      // Ghost material should be in mockBasicMaterials (created by createGhostMaterial)
      expect(mockBasicMaterials.some((m: any) => m === ghostMesh.material)).toBe(true);
      // Should NOT be a standard material
      expect(mockMaterials.some((m: any) => m === ghostMesh.material)).toBe(false);
    });

    it('(d) setVisibility show removes ghost clone from ghostGroup and re-adds mesh to scene', () => {
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');
      vi.clearAllMocks();

      (manager as any).setVisibility('A', 'show');

      const mesh = manager.getSceneMeshes().get('A')!;
      expect(mockGroupRemove).toHaveBeenCalledTimes(1);
      expect(mockSceneAdd).toHaveBeenCalledWith(mesh);
    });

    it('(e) setVisibility hidden removes mesh from scene and not added to ghostGroup', () => {
      const { manager } = setupWithMesh();
      const mesh = manager.getSceneMeshes().get('A')!;

      (manager as any).setVisibility('A', 'hidden');

      expect(mockSceneRemove).toHaveBeenCalledWith(mesh);
      expect(mockGroupAdd).not.toHaveBeenCalled(); // hidden means not in ghostGroup
    });

    it('(e) setVisibility hidden from ghost state removes ghost clone from ghostGroup', () => {
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');
      vi.clearAllMocks();

      (manager as any).setVisibility('A', 'hidden');

      expect(mockGroupRemove).toHaveBeenCalledTimes(1);
      expect(mockSceneAdd).not.toHaveBeenCalled();
    });

    it('(f) getSceneMeshes only returns show meshes, not ghost or hidden', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      manager.sync({
        A: makeMeshData('A'),
        B: makeMeshData('B'),
        C: makeMeshData('C'),
      });

      (manager as any).setVisibility('B', 'ghost');
      (manager as any).setVisibility('C', 'hidden');

      const sceneMeshes = manager.getSceneMeshes();
      expect(sceneMeshes.has('A')).toBe(true);
      expect(sceneMeshes.has('B')).toBe(false);
      expect(sceneMeshes.has('C')).toBe(false);
      expect(sceneMeshes.size).toBe(1);
    });

    it('(g) getGhostMeshes returns the ghost mesh map', () => {
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');

      const ghostMeshes: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMeshes).toBeInstanceOf(Map);
      expect(ghostMeshes.has('A')).toBe(true);
      expect(ghostMeshes.size).toBe(1);
    });

    it('(g) getGhostMeshes is empty when no ghost entities', () => {
      const { manager } = setupWithMesh();
      const ghostMeshes: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMeshes.size).toBe(0);
    });

    it('(h) sync respects pre-set ghost visibility for newly arriving mesh', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      // Pre-set visibility BEFORE mesh data arrives
      (manager as any).setVisibility('A', 'ghost');

      vi.clearAllMocks();
      manager.sync({ A: makeMeshData('A') });

      // Mesh should NOT be added to scene (it's ghost)
      expect(mockSceneAdd).not.toHaveBeenCalled();
      // Ghost clone should be added to ghostGroup
      expect(mockGroupAdd).toHaveBeenCalledTimes(1);
      // getSceneMeshes should NOT include A
      expect(manager.getSceneMeshes().has('A')).toBe(false);
      // getGhostMeshes should include A
      expect((manager as any).getGhostMeshes().has('A')).toBe(true);
    });

    it('(i) sync removal cleans up ghost meshes when entity removed while ghosted', () => {
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');
      vi.clearAllMocks();

      // Remove A from sync
      manager.sync({});

      // Ghost clone should be removed from ghostGroup
      expect(mockGroupRemove).toHaveBeenCalledTimes(1);
      expect((manager as any).getGhostMeshes().has('A')).toBe(false);
    });

    it('(j) dispose cleans up ghost group, ghost meshes, and ghost material', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      manager.sync({ A: makeMeshData('A') });
      (manager as any).setVisibility('A', 'ghost');
      vi.clearAllMocks();

      manager.dispose();

      // Ghost material should be disposed
      expect(mockBasicMaterials.some((m: any) => m.dispose.mock.calls.length > 0)).toBe(true);
      // Ghost map should be empty
      expect((manager as any).getGhostMeshes().size).toBe(0);
    });

    it('(k) ghost → hidden → show round-trip restores mesh to scene', () => {
      const { manager } = setupWithMesh();

      (manager as any).setVisibility('A', 'ghost');
      expect(manager.getSceneMeshes().has('A')).toBe(false);
      expect((manager as any).getGhostMeshes().has('A')).toBe(true);

      (manager as any).setVisibility('A', 'hidden');
      expect(manager.getSceneMeshes().has('A')).toBe(false);
      expect((manager as any).getGhostMeshes().has('A')).toBe(false);

      vi.clearAllMocks();
      (manager as any).setVisibility('A', 'show');
      expect(manager.getSceneMeshes().has('A')).toBe(true);
      expect((manager as any).getGhostMeshes().has('A')).toBe(false);
      // Mesh re-added to scene
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
    });

    it('(l) ghost→ghost idempotency: second setVisibility ghost is a no-op', () => {
      const { manager } = setupWithMesh();

      // First ghost transition: removes mesh from scene, adds ghost clone
      (manager as any).setVisibility('A', 'ghost');
      vi.clearAllMocks();

      // Second ghost call: early-return when prevState === state (idempotency guard)
      (manager as any).setVisibility('A', 'ghost');

      // No scene or group operations should have occurred
      expect(mockSceneRemove).not.toHaveBeenCalled();
      expect(mockGroupAdd).not.toHaveBeenCalled();
      // Ghost map still has exactly one entry
      expect((manager as any).getGhostMeshes().size).toBe(1);
      // Mesh is not in scene
      expect(manager.getSceneMeshes().has('A')).toBe(false);
    });

    it('(l2) show→show idempotency: second setVisibility show on visible mesh is a no-op', () => {
      const { manager } = setupWithMesh();
      // Mesh starts in 'show' state after sync
      vi.clearAllMocks();

      // Calling show on an already-visible mesh: early-return when prevState === state
      (manager as any).setVisibility('A', 'show');

      // No scene operations should have occurred
      expect(mockSceneAdd).not.toHaveBeenCalled();
      expect(mockSceneRemove).not.toHaveBeenCalled();
      expect(mockGroupAdd).not.toHaveBeenCalled();
      // Mesh still in scene, not in ghost map
      expect(manager.getSceneMeshes().has('A')).toBe(true);
      expect((manager as any).getGhostMeshes().has('A')).toBe(false);
    });

    it('(m) orphan visibility: setVisibility on never-synced entity stores state without error', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Set visibility on an entity that has never been synced
      expect(() => (manager as any).setVisibility('Z', 'ghost')).not.toThrow();

      // No mesh exists, so no scene or group operations occur
      expect(mockSceneRemove).not.toHaveBeenCalled();
      expect(mockGroupAdd).not.toHaveBeenCalled();
      // Entity is not in ghost or scene map (no mesh was created)
      expect((manager as any).getGhostMeshes().has('Z')).toBe(false);
      expect(manager.getSceneMeshes().has('Z')).toBe(false);

      // Sync with empty record: orphan visibility state should not leak or cause errors
      expect(() => manager.sync({})).not.toThrow();
      expect((manager as any).getGhostMeshes().has('Z')).toBe(false);
      expect(manager.getSceneMeshes().has('Z')).toBe(false);
    });

    it('(n) orphan visibility: pre-stored ghost state is applied when entity later arrives via sync', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Pre-store ghost visibility before the entity has any mesh data
      (manager as any).setVisibility('Z', 'ghost');

      // Entity arrives via sync: pre-stored state is read and ghost branch is taken,
      // so addGhostClone is called instead of scene.add
      manager.sync({ Z: makeMeshData('Z') });

      // Entity should be in ghost map (ghost clone was added), not scene map
      expect((manager as any).getGhostMeshes().has('Z')).toBe(true);
      expect(manager.getSceneMeshes().has('Z')).toBe(false);
      // Ghost clone was added to ghostGroup, not directly to scene
      expect(mockGroupAdd).toHaveBeenCalledTimes(1);
      expect(mockSceneAdd).not.toHaveBeenCalledWith(expect.objectContaining({ name: 'Z' }));
    });

    it('(o) orphan visibility: pre-stored show state results in normal scene-add on sync', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Pre-store show visibility before any mesh data arrives
      (manager as any).setVisibility('Z', 'show');

      // Entity arrives via sync: show is the default path, mesh should be scene-added normally
      manager.sync({ Z: makeMeshData('Z') });

      // Mesh should be in scene map, not ghost map
      expect(manager.getSceneMeshes().has('Z')).toBe(true);
      expect((manager as any).getGhostMeshes().has('Z')).toBe(false);
      // scene.add was called for the mesh
      expect(mockSceneAdd).toHaveBeenCalled();
      expect(mockGroupAdd).not.toHaveBeenCalled();
    });

    it('(p) orphan visibility: pre-stored hidden state suppresses scene and ghost placement on sync', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Pre-store hidden visibility before any mesh data arrives
      (manager as any).setVisibility('Z', 'hidden');

      // Entity arrives via sync: hidden branch means mesh is created but placed nowhere
      manager.sync({ Z: makeMeshData('Z') });

      // Mesh should have been created (it exists internally) but neither in scene nor ghost map
      expect(manager.getSceneMeshes().has('Z')).toBe(false);
      expect((manager as any).getGhostMeshes().has('Z')).toBe(false);
      // Neither scene.add nor ghostGroup.add should have been called for 'Z'
      expect(mockGroupAdd).not.toHaveBeenCalled();
      expect(mockSceneAdd).not.toHaveBeenCalledWith(expect.objectContaining({ name: 'Z' }));
    });

    it('(q) orphan visibilityMap pre-set is pruned by sync({}) before mesh arrives', () => {
      // S1: if setVisibility is called before the mesh exists and the entity never
      // arrives in a sync, the pre-set should be pruned when sync() is called with
      // a set that does not include the entity.
      const scene = new Scene();
      const manager = createMeshManager(scene);

      // Pre-set ghost for an entity that hasn't arrived yet
      (manager as any).setVisibility('orphan', 'ghost');

      // Sync with empty set — 'orphan' is absent, so the pre-set is now an orphan
      manager.sync({});

      vi.clearAllMocks();
      // Now sync with the entity — because the pre-set was pruned, it should arrive as 'show'
      manager.sync({ orphan: makeMeshData('orphan') });

      // Mesh should be added to scene (show), not as a ghost clone
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
      expect(mockGroupAdd).not.toHaveBeenCalled();
      expect(manager.getSceneMeshes().has('orphan')).toBe(true);
      expect((manager as any).getGhostMeshes().has('orphan')).toBe(false);
    });

    it('(s) hidden visibilityMap entry does not survive removal so re-arrival defaults to show', () => {
      // Covers the arrival → removal → re-arrival path for the 'hidden' state.
      // Phase 1: pre-set hidden, entity arrives hidden (no scene.add or ghostGroup.add).
      // Phase 2: sync({}) removes the mesh — removeMesh() deletes the visibilityMap entry.
      // Phase 3: entity re-arrives — because the hidden entry was deleted by removeMesh,
      //          no pre-set remains and it should arrive as 'show' (added to scene).
      // Note: the visibilityMap deletion is done by removeMesh(), not the orphan-prune loop.
      // See test (t) for the path where sync({}) prunes a hidden pre-set before any arrival.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Pre-set hidden before the entity has ever arrived
      (manager as any).setVisibility('hidden-orphan', 'hidden');

      // Entity arrives for the first time: should be hidden (not added to scene or ghost group)
      manager.sync({ 'hidden-orphan': makeMeshData('hidden-orphan') });
      expect(mockSceneAdd).not.toHaveBeenCalledWith(expect.objectContaining({ name: 'hidden-orphan' }));
      expect(mockGroupAdd).not.toHaveBeenCalled();

      // Sync with empty set — mesh is removed; removeMesh() deletes the visibilityMap entry
      manager.sync({});

      vi.clearAllMocks();

      // Re-arrive: because the hidden entry was deleted by removeMesh, entity arrives as 'show'
      manager.sync({ 'hidden-orphan': makeMeshData('hidden-orphan') });

      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
      expect(mockGroupAdd).not.toHaveBeenCalled();
      expect(manager.getSceneMeshes().has('hidden-orphan')).toBe(true);
      expect((manager as any).getGhostMeshes().has('hidden-orphan')).toBe(false);
    });

    it('(t) hidden orphan visibilityMap pre-set is pruned by sync({}) before mesh arrives', () => {
      // Symmetric counterpart to test (q) for the 'hidden' state.
      // If setVisibility('hidden') is called before the mesh exists and the entity never
      // arrives in a sync, the pre-set should be pruned when sync() is called with a set
      // that does not include the entity (the orphan-prune block at the end of sync()).
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Pre-set hidden for an entity that hasn't arrived yet
      (manager as any).setVisibility('hidden-orphan', 'hidden');

      // Sync with empty set — 'hidden-orphan' is absent, so the pre-set is now an orphan
      manager.sync({});

      vi.clearAllMocks();
      // Now sync with the entity — because the orphan pre-set was pruned, it should arrive as 'show'
      manager.sync({ 'hidden-orphan': makeMeshData('hidden-orphan') });

      // Mesh should be added to scene (show), not hidden or ghost
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
      expect(mockGroupAdd).not.toHaveBeenCalled();
      expect(manager.getSceneMeshes().has('hidden-orphan')).toBe(true);
      expect((manager as any).getGhostMeshes().has('hidden-orphan')).toBe(false);
    });

    it('(r) getGhostMeshes returns a copy — external .delete/.set/.clear do not affect internal state', () => {
      // S2: getGhostMeshes must return a defensive copy so callers cannot accidentally
      // mutate the internal ghostMeshMap via .delete(), .set(), or .clear().
      const { manager } = setupWithMesh();
      (manager as any).setVisibility('A', 'ghost');

      // --- .delete() does not affect internal state ---
      const ghostMap1: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMap1.has('A')).toBe(true);
      ghostMap1.delete('A');

      // Internal state must be unaffected — a second call still returns 'A'
      const ghostMap2: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMap2.has('A')).toBe(true);
      expect(ghostMap2.size).toBe(1);

      // --- .set() does not affect internal state ---
      ghostMap2.set('injected', {} as any);

      // A fresh call must not contain 'injected' and must still have only 'A'
      const ghostMap3: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMap3.has('injected')).toBe(false);
      expect(ghostMap3.has('A')).toBe(true);
      expect(ghostMap3.size).toBe(1);

      // --- .clear() does not affect internal state ---
      const ghostMap4: Map<string, any> = (manager as any).getGhostMeshes();
      ghostMap4.clear();

      // A fresh call must still contain 'A'
      const ghostMap5: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMap5.has('A')).toBe(true);
      expect(ghostMap5.size).toBe(1);
    });
  });

  describe('end-to-end pipeline: RawMeshData → convertRawMesh → meshManager (E2E-01)', () => {
    it('full IPC→TS→meshManager pipeline produces phong material and colour buffer', () => {
      // Construct a RawMeshData JSON literal that mimics what the Rust serializer emits.
      // (3 vertices: a right-angle triangle in the XY plane)
      const raw: RawMeshData = {
        entity_path: 'B',
        vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
        indices: [0, 1, 2],
        normals: null,
        scalar_channels: { vonMises: [10, 20, 30] },
        displaced_positions: [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0],
      };

      // Step 1: convert raw payload → typed MeshData
      const converted = convertRawMesh(raw);

      // Verify conversion preserved scalar_channels as Float32Array
      expect(converted.scalar_channels).toBeDefined();
      expect(converted.scalar_channels!.vonMises).toBeInstanceOf(Float32Array);
      expect(Array.from(converted.scalar_channels!.vonMises)).toEqual([10, 20, 30]);

      // Verify displaced_positions was converted
      expect(converted.displaced_positions).toBeInstanceOf(Float32Array);

      // Step 2: feed through meshManager with colorize
      const sentinelBake = (s: Float32Array) =>
        new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);

      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      manager.sync({ B: converted });

      const mesh = manager.getSceneMeshes().get('B')!;
      expect(mesh).toBeDefined();

      // Material is MeshPhongMaterial with vertexColors
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(true);
      const mat = mesh.material as any;
      expect(mat.vertexColors).toBe(true);
      expect(mat.flatShading).toBe(false);
      expect(mat.side).toBe(2); // DoubleSide

      // Geometry has color attribute baked by sentinelBake([10, 20, 30])
      const colorAttr = (mesh.geometry as any).attributes.color;
      expect(colorAttr).toBeDefined();
      expect(colorAttr.itemSize).toBe(3);
      expect(Array.from(colorAttr.array as Float32Array)).toEqual([10, 0, 0, 20, 0, 0, 30, 0, 0]);

      // displaced_positions is preserved in the converted MeshData but only applied to the
      // position buffer when `setDeformation` is active (see deformation E2E below).
      // Before setDeformation is called, the position buffer must equal the original vertices.
      const posAttr = (mesh.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    });
  });

  describe('end-to-end pipeline: setDeformation full-pipeline pin (E2E-02)', () => {
    it('before setDeformation: position = original; after W=10: blended + overlay; after null: restored', () => {
      const raw: RawMeshData = {
        entity_path: 'B',
        vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
        indices: [0, 1, 2],
        normals: null,
        displaced_positions: [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0],
      };

      const converted = convertRawMesh(raw);
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.sync({ B: converted });

      const mesh = manager.getSceneMeshes().get('B')!;
      expect(mesh).toBeDefined();

      // (a) Before setDeformation: position equals original vertices.
      const posAttr = (mesh.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);

      // (b) After setDeformation({warpFactor:10}): position is W=10 blend.
      // Compute expected using the CONVERTED Float32Array values (matches f32 rounding in applyWarpToMesh).
      const origF32 = converted.vertices;
      const dispF32 = converted.displaced_positions!;
      const expectedW10 = new Float32Array(origF32.length);
      for (let i = 0; i < origF32.length; i++) {
        expectedW10[i] = origF32[i] + 10 * (dispF32[i] - origF32[i]);
      }
      manager.setDeformation({ warpFactor: 10 });
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(expectedW10));

      // Overlay exists and holds the original vertices.
      const overlays = manager.getDeformedOverlays();
      expect(overlays.size).toBe(1);
      expect(overlays.has('B')).toBe(true);
      const overlayPosAttr = (overlays.get('B')!.geometry as any).attributes.position;
      expect(Array.from(overlayPosAttr.array as Float32Array)).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);

      // (c) After setDeformation(null): position restored to original; no overlays.
      manager.setDeformation(null);
      expect(Array.from(posAttr.array as Float32Array)).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      expect(manager.getDeformedOverlays().size).toBe(0);
    });
  });

  describe('rebuildMaterials — colorize=null path', () => {
    const sentinelBake = (s: Float32Array) =>
      new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);

    function setupColorizedMesh() {
      const scene = new Scene();
      const manager = createMeshManager(scene, {
        colorize: { channel: 'vonMises', bake: sentinelBake },
      });
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
      manager.sync({ A: meshData });
      return { scene, manager };
    }

    it('(a) after setColorize(null)+rebuildMaterials, mesh material is MeshStandardMaterial', () => {
      const { manager } = setupColorizedMesh();
      manager.setColorize(null);
      manager.rebuildMaterials();

      const mesh = manager.getSceneMeshes().get('A')!;
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(false);
    });

    it('(b) previous MeshPhongMaterial dispose was called once', () => {
      const { manager } = setupColorizedMesh();
      const mesh = manager.getSceneMeshes().get('A')!;
      const oldMaterial = mesh.material as any;
      // Confirm it's currently a phong material
      expect(mockPhongMaterials.some((m: any) => m === oldMaterial)).toBe(true);

      manager.setColorize(null);
      manager.rebuildMaterials();

      expect(oldMaterial.dispose).toHaveBeenCalledOnce();
    });

    it('(c) geometry color BufferAttribute is removed after rebuildMaterials', () => {
      const { manager } = setupColorizedMesh();
      const mesh = manager.getSceneMeshes().get('A')!;
      // Confirm color attr present before rebuild
      expect((mesh.geometry as any).attributes.color).toBeDefined();

      manager.setColorize(null);
      manager.rebuildMaterials();

      expect((mesh.geometry as any).getAttribute('color')).toBeUndefined();
    });

    it('(d) BVH computeBoundsTree is NOT called again during rebuildMaterials', () => {
      const { manager } = setupColorizedMesh();
      vi.clearAllMocks(); // reset call counts after sync

      manager.setColorize(null);
      manager.rebuildMaterials();

      expect(mockComputeBoundsTree).not.toHaveBeenCalled();
    });

    it('(e) ghost-clone visibility state is unchanged by rebuildMaterials', () => {
      const { manager } = setupColorizedMesh();
      // No ghost clones exist; verify getGhostMeshes is empty after rebuild
      manager.setColorize(null);
      manager.rebuildMaterials();

      const ghostMap: Map<string, any> = (manager as any).getGhostMeshes();
      expect(ghostMap.size).toBe(0);
    });
  });

  describe('rebuildMaterials — colorize=set path', () => {
    const sentinelBake = (s: Float32Array) =>
      new Float32Array([s[0], 0, 0, s[1], 0, 0, s[2], 0, 0]);

    function setupStandardMeshWithScalars() {
      // Create manager WITHOUT colorize → mesh gets MeshStandardMaterial, no color attr
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
      };
      manager.sync({ A: meshData });
      return { scene, manager };
    }

    it('(a) after setColorize+rebuildMaterials, mesh material is MeshPhongMaterial with vertexColors', () => {
      const { manager } = setupStandardMeshWithScalars();
      const mesh = manager.getSceneMeshes().get('A')!;
      // Confirm starts as standard material (no colorize at creation)
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(false);

      manager.setColorize({ channel: 'vonMises', bake: sentinelBake });
      manager.rebuildMaterials();

      // Material should now be MeshPhongMaterial
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(false);
      // With vertexColors = true
      expect((mesh.material as any).vertexColors).toBe(true);
    });

    it('(b) previous MeshStandardMaterial dispose was called', () => {
      const { manager } = setupStandardMeshWithScalars();
      const mesh = manager.getSceneMeshes().get('A')!;
      const oldMaterial = mesh.material as any;
      expect(mockMaterials.some((m: any) => m === oldMaterial)).toBe(true);

      manager.setColorize({ channel: 'vonMises', bake: sentinelBake });
      manager.rebuildMaterials();

      expect(oldMaterial.dispose).toHaveBeenCalledOnce();
    });

    it('(c) color BufferAttribute is present with baked scalar values', () => {
      const { manager } = setupStandardMeshWithScalars();
      const mesh = manager.getSceneMeshes().get('A')!;
      // Confirm no color attr before rebuild
      expect((mesh.geometry as any).attributes.color).toBeUndefined();

      manager.setColorize({ channel: 'vonMises', bake: sentinelBake });
      manager.rebuildMaterials();

      // sentinelBake([10, 20, 30]) = [10, 0, 0, 20, 0, 0, 30, 0, 0]
      const colorAttr = (mesh.geometry as any).attributes.color;
      expect(colorAttr).toBeDefined();
      expect(colorAttr.itemSize).toBe(3);
      expect(Array.from(colorAttr.array as Float32Array)).toEqual([10, 0, 0, 20, 0, 0, 30, 0, 0]);
    });

    it('(d) channel-absent fallback: mesh missing the colorize channel keeps MeshStandardMaterial', () => {
      // Create manager without colorize, sync mesh with displacement_magnitude only (no vonMises)
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      const meshData: MeshData = {
        entity_path: 'B',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        scalar_channels: { displacement_magnitude: new Float32Array([0.1, 0.2, 0.3]) },
      };
      manager.sync({ B: meshData });

      const mesh = manager.getSceneMeshes().get('B')!;
      const oldMaterial = mesh.material as any;
      expect(mockMaterials.some((m: any) => m === oldMaterial)).toBe(true);

      // setColorize for vonMises, but this mesh only has displacement_magnitude
      manager.setColorize({ channel: 'vonMises', bake: sentinelBake });
      manager.rebuildMaterials();

      // Still MeshStandardMaterial (channel absent → fallback path)
      expect(mockMaterials.some((m: any) => m === mesh.material)).toBe(true);
      expect(mockPhongMaterials.some((m: any) => m === mesh.material)).toBe(false);
      // No color attribute on geometry
      expect((mesh.geometry as any).attributes.color).toBeUndefined();
      // Old material was disposed (replaced with a fresh standard material)
      expect(oldMaterial.dispose).toHaveBeenCalledOnce();
    });

    it('(e) BVH computeBoundsTree is NOT called during rebuildMaterials colorize=set path', () => {
      const { manager } = setupStandardMeshWithScalars();
      vi.clearAllMocks(); // reset after sync

      manager.setColorize({ channel: 'vonMises', bake: sentinelBake });
      manager.rebuildMaterials();

      expect(mockComputeBoundsTree).not.toHaveBeenCalled();
    });
  });

  describe('setDeformation — null / idempotent / double-call', () => {
    it('(a) setDeformation(null) after setDeformation({warpFactor:1}) restores position.array to original vertices', () => {
      const { manager, vertices } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 1 });
      manager.setDeformation(null);

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertices));
      expect(posAttr.needsUpdate).toBe(true);
    });

    it('(b) setDeformation(null) when no prior deformation is a no-op (no error, position unchanged)', () => {
      const { manager } = setupDeformableMesh();
      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      const originalValues = Array.from(posAttr.array as Float32Array);

      // Should not throw; position should remain as original
      expect(() => manager.setDeformation(null)).not.toThrow();
      expect(Array.from(posAttr.array as Float32Array)).toEqual(originalValues);
    });

    it('(c) calling setDeformation twice with different warps replaces cleanly (W=1 then W=10 → W=10 positions)', () => {
      const { manager, vertices, displaced } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 1 });

      const expectedW10 = new Float32Array(vertices.length);
      for (let i = 0; i < vertices.length; i++) {
        expectedW10[i] = vertices[i] + 10 * (displaced[i] - vertices[i]);
      }

      manager.setDeformation({ warpFactor: 10 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      // Must reflect W=10, not accumulated or stuck at W=1.
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(expectedW10));
    });
  });

  describe('undeformed overlay', () => {
    function setupWithOverlay() {
      return setupDeformableMesh({
        B: {
          entity_path: 'B',
          vertices: new Float32Array([2, 0, 0, 3, 0, 0, 2, 1, 0]),
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        },
      });
    }

    it('(a) getDeformedOverlays() returns empty Map before setDeformation', () => {
      const { manager } = setupWithOverlay();
      expect(manager.getDeformedOverlays().size).toBe(0);
    });

    it('(b) after setDeformation({warpFactor:5}), getDeformedOverlays() has size 1 with key "A"', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const overlays = manager.getDeformedOverlays();
      expect(overlays.size).toBe(1);
      expect(overlays.has('A')).toBe(true);
    });

    it('(b) overlay material is transparent with opacity 0.25 and depthWrite false', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const overlay = manager.getDeformedOverlays().get('A')!;
      expect(overlay.material).toBeDefined();
      const mat = overlay.material as any;
      expect(mat.transparent).toBe(true);
      expect(mat.opacity).toBe(0.25);
      expect(mat.depthWrite).toBe(false);
    });

    it('(b) overlay renderOrder is less than deformed mesh renderOrder (overlay behind)', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const deformedMesh = manager.getSceneMeshes().get('A')!;
      const overlay = manager.getDeformedOverlays().get('A')!;
      expect(overlay.renderOrder).toBeLessThan(deformedMesh.renderOrder === undefined ? 0 : deformedMesh.renderOrder + 1);
      expect(overlay.renderOrder).toBe(-1);
    });

    it('(b) overlay position.array equals original vertices (NOT warped)', () => {
      const { manager, vertices } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const overlay = manager.getDeformedOverlays().get('A')!;
      const posAttr = (overlay.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertices));
    });

    it('(c) mesh B (no displaced_positions) gets no overlay — size remains 1', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      expect(manager.getDeformedOverlays().size).toBe(1);
      expect(manager.getDeformedOverlays().has('B')).toBe(false);
    });

    // --- step-13 teardown tests ---

    it('(d) setDeformation(null) removes overlay from undeformedGroup and disposes its geometry', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const overlay = manager.getDeformedOverlays().get('A')!;
      const overlayGeom = overlay.geometry;

      vi.clearAllMocks();
      manager.setDeformation(null);

      // The overlay mesh must have been removed from the group.
      expect(mockGroupRemove).toHaveBeenCalledWith(overlay);
      // The overlay's own BufferGeometry must have been disposed.
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
      // The overlays map is now empty.
      expect(manager.getDeformedOverlays().size).toBe(0);
    });

    it('(e) calling setDeformation({warpFactor:5}) twice with the same warpFactor is a no-op — overlay unchanged', () => {
      // setDeformation detects same warpFactor and returns early, avoiding unnecessary
      // GPU-buffer writes and overlay teardown/re-add on redundant calls.
      // The Viewport bridge effect guards against this via track-then-act reactive
      // tracking, but the public API must be stable on its own.
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      // Capture first overlay's geometry before the second call.
      const firstOverlay = manager.getDeformedOverlays().get('A')!;
      const firstGeom = firstOverlay.geometry;

      vi.clearAllMocks();
      manager.setDeformation({ warpFactor: 5 });

      // Still exactly one overlay.
      expect(manager.getDeformedOverlays().size).toBe(1);
      // Early return: the prior overlay's geometry must NOT have been disposed.
      expect(firstGeom.dispose).not.toHaveBeenCalled();
      // The overlay object is the same instance (no teardown/re-create).
      expect(manager.getDeformedOverlays().get('A')).toBe(firstOverlay);
    });

    it('(f) dispose() removes undeformedGroup from scene and disposes undeformedMaterial', () => {
      const { manager } = setupWithOverlay();
      manager.setDeformation({ warpFactor: 5 });

      const overlay = manager.getDeformedOverlays().get('A')!;
      const overlayGeom = overlay.geometry;

      // mockGroups[1] == undeformedGroup; mockBasicMaterials[1] == undeformedMaterial.
      // Both are created inside createMeshManager (after ghostGroup/ghostMaterial).
      const undeformedGroup = mockGroups[1];
      const undeformedMaterial = mockBasicMaterials[1];

      vi.clearAllMocks();
      manager.dispose();

      // All overlays cleaned up.
      expect(manager.getDeformedOverlays().size).toBe(0);
      // Overlay geometry was disposed.
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
      // undeformedGroup was removed from the scene.
      expect(mockSceneRemove).toHaveBeenCalledWith(undeformedGroup);
      // undeformedMaterial was disposed.
      expect(undeformedMaterial.dispose).toHaveBeenCalledOnce();
    });

    // --- step-15: entity removal while deformation is active ---

    it('(g) sync({}) while deformation active removes and disposes overlay for removed entity', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      const meshA: MeshData = {
        entity_path: 'A',
        vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        displaced_positions: new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]),
      };

      manager.sync({ A: meshA });
      manager.setDeformation({ warpFactor: 5 });

      // Capture overlay reference for later disposal check.
      const overlay = manager.getDeformedOverlays().get('A')!;
      expect(overlay).toBeDefined();
      const overlayGeom = overlay.geometry;

      vi.clearAllMocks();
      // Remove the entity.
      manager.sync({});

      // Overlay must be gone.
      expect(manager.getDeformedOverlays().size).toBe(0);
      // Overlay geometry must have been disposed.
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
      // Overlay mesh must have been removed from undeformedGroup.
      expect(mockGroupRemove).toHaveBeenCalledWith(overlay);
    });

    it('(i) setVisibility("A","hidden") before setDeformation — overlay is NOT added for hidden entity', () => {
      // Pins the setDeformation visibility gate at meshManager.ts (isShown helper) —
      // hidden entities must not gain an overlay.
      const { manager } = setupWithOverlay();
      manager.setVisibility('A', 'hidden');
      manager.setDeformation({ warpFactor: 5 });
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
    });

    it('(j) setVisibility("A","ghost") before setDeformation — overlay is NOT added for ghost entity', () => {
      // Locks in the UX decision: ghost is a translucent deformed rendering; a
      // separate undeformed overlay would be redundant and visually noisy.
      const { manager } = setupWithOverlay();
      manager.setVisibility('A', 'ghost');
      manager.setDeformation({ warpFactor: 5 });
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
    });

    it('(h) overlay owns clones of index/normal — deformed mesh references are not aliased and survive overlay teardown', () => {
      const { manager } = setupWithOverlay();

      // Snapshot deformed mesh's index and normal references BEFORE any deformation call.
      const deformedMesh = manager.getSceneMeshes().get('A')!;
      const deformedIndexRef = (deformedMesh.geometry as any).index;
      const deformedNormalRef = (deformedMesh.geometry as any).attributes.normal;

      manager.setDeformation({ warpFactor: 1 });

      const overlay = manager.getDeformedOverlays().get('A')!;

      // Overlay must NOT share BufferAttribute references with the deformed mesh (clone semantics).
      expect((overlay.geometry as any).index).not.toBe(deformedIndexRef);
      expect((overlay.geometry as any).attributes.normal).not.toBe(deformedNormalRef);

      // Cloned arrays must carry the same content as the originals (data integrity).
      expect(Array.from((overlay.geometry as any).index.array as Uint32Array)).toEqual(
        Array.from(deformedIndexRef.array as Uint32Array)
      );
      expect(Array.from((overlay.geometry as any).attributes.normal.array as Float32Array)).toEqual(
        Array.from(deformedNormalRef.array as Float32Array)
      );

      // Tear down the overlay.
      manager.setDeformation(null);

      // Deformed mesh's index and normal must still be the SAME objects as before
      // (i.e., overlay disposal did not free or replace the deformed mesh's VBOs).
      expect((deformedMesh.geometry as any).index).toBe(deformedIndexRef);
      expect((deformedMesh.geometry as any).attributes.normal).toBe(deformedNormalRef);
      // Disposal-isolation contract: overlay teardown must not call .dispose() on the
      // deformed mesh's own index/normal attribute objects (distinct from the overlay's
      // cloned copies, whose dispose is expected to be called via overlay.geometry.dispose()).
      expect(deformedIndexRef.dispose).not.toHaveBeenCalled();
      expect(deformedNormalRef.dispose).not.toHaveBeenCalled();
    });
  });

  describe('setVisibility — overlay visibility gating', () => {
    const vertices = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const displaced = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);

    function makeDisplacedMesh(entityPath: string): MeshData {
      return {
        entity_path: entityPath,
        vertices: vertices.slice(),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        displaced_positions: displaced.slice(),
      };
    }

    // --- step-05: createMeshFromData mid-stream path ---

    it('(a) mid-stream sync of hidden entity after setDeformation — no overlay added', () => {
      // Pins the createMeshFromData add-path gate (line ~244-247).
      // Entity C is pre-set hidden, then deformation is enabled, then C arrives via sync.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.setDeformation({ warpFactor: 5 });
      manager.setVisibility('C', 'hidden');
      manager.sync({ C: makeDisplacedMesh('C') });
      expect(manager.getDeformedOverlays().has('C')).toBe(false);
      expect(manager.getDeformedOverlays().size).toBe(0);
    });

    // --- step-07: updateMeshGeometry re-sync path ---

    it('(b) re-sync of hidden entity while deformation active — overlay stays absent', () => {
      // Pins the updateMeshGeometry gate (line ~330-342).
      // A is pre-set hidden, synced (no overlay added), deformation enabled (still no overlay),
      // then A is re-synced with new vertices — removeUndeformedOverlay is a no-op but
      // addUndeformedOverlay must still be suppressed.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.setVisibility('A', 'hidden');
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      // Confirm no overlay yet.
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
      // Re-sync with new displaced data.
      manager.sync({ A: { ...makeDisplacedMesh('A'), vertices: new Float32Array([0, 0, 0, 2, 0, 0, 0, 2, 0]), displaced_positions: new Float32Array([0.5, 0, 0, 2.5, 0, 0, 0.5, 2, 0]) } });
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
      expect(manager.getDeformedOverlays().size).toBe(0);
    });

    // --- step-09: show→hidden removes overlay ---

    it('(c) show→hidden while deformation active — overlay removed and disposed', () => {
      // Pins the setVisibility show→hidden branch: must call removeUndeformedOverlay.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      const overlay = manager.getDeformedOverlays().get('A')!;
      expect(overlay).toBeDefined();
      const overlayGeom = overlay.geometry;
      vi.clearAllMocks();
      manager.setVisibility('A', 'hidden');
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(mockGroupRemove).toHaveBeenCalledWith(overlay);
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
    });

    // --- step-11: show→ghost removes overlay ---

    it('(d) show→ghost while deformation active — overlay removed, ghost clone added', () => {
      // Pins the setVisibility show→ghost branch: must call removeUndeformedOverlay.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      const overlay = manager.getDeformedOverlays().get('A')!;
      const overlayGeom = overlay.geometry;
      vi.clearAllMocks();
      manager.setVisibility('A', 'ghost');
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(mockGroupRemove).toHaveBeenCalledWith(overlay);
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
    });

    // --- step-13: hidden→show adds overlay ---

    it('(e) hidden→show while deformation active — overlay is created with original vertices', () => {
      // Pins the setVisibility hidden→show branch: must call addUndeformedOverlay.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.setVisibility('A', 'hidden');
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      // Confirm no overlay yet (A is hidden).
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
      manager.setVisibility('A', 'show');
      expect(manager.getDeformedOverlays().has('A')).toBe(true);
      const overlay = manager.getDeformedOverlays().get('A')!;
      const posAttr = (overlay.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertices));
      expect(overlay.renderOrder).toBe(-1);
    });

    // --- step-15: ghost→show adds overlay ---

    it('(f) ghost→show while deformation active — overlay is created with original vertices', () => {
      // Pins the setVisibility ghost→show branch: must call addUndeformedOverlay.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      // overlay present while show
      expect(manager.getDeformedOverlays().has('A')).toBe(true);
      manager.setVisibility('A', 'ghost');
      // overlay gone while ghost
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
      manager.setVisibility('A', 'show');
      // overlay restored
      expect(manager.getDeformedOverlays().has('A')).toBe(true);
      const overlay = manager.getDeformedOverlays().get('A')!;
      const posAttr = (overlay.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertices));
    });

    // --- hidden↔ghost no-op cases (overlay is absent in both states) ---

    it('(g) hidden→ghost while deformation active — overlay remains absent throughout', () => {
      // Both hidden and ghost states have no overlay. Transitioning between them is a
      // no-op for overlay machinery and must not produce or remove anything.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.setVisibility('A', 'hidden');
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      // A is hidden — no overlay.
      expect(manager.getDeformedOverlays().size).toBe(0);
      manager.setVisibility('A', 'ghost');
      // A is now ghost — still no overlay.
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
    });

    it('(h) ghost→hidden while deformation active — overlay remains absent throughout', () => {
      // Symmetric of (g): going ghost→hidden must not add an overlay.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.sync({ A: makeDisplacedMesh('A') });
      manager.setDeformation({ warpFactor: 5 });
      // A starts show — overlay present.
      expect(manager.getDeformedOverlays().has('A')).toBe(true);
      manager.setVisibility('A', 'ghost');
      // overlay gone after show→ghost.
      expect(manager.getDeformedOverlays().size).toBe(0);
      manager.setVisibility('A', 'hidden');
      // overlay still absent after ghost→hidden.
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
    });

    // --- deformation toggled off mid-transition ---

    it('(i2) hidden→show when deformation is OFF — overlay must NOT be added', () => {
      // Gate (a): currentDeformation === null → addUndeformedOverlay must not be called.
      // Even if the entity has displaced_positions, no overlay is created without an active
      // deformation config (there is nothing meaningful to show as the undeformed shape).
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();
      manager.setVisibility('A', 'hidden');
      manager.sync({ A: makeDisplacedMesh('A') });
      // Turn deformation ON then OFF so currentDeformation returns to null.
      manager.setDeformation({ warpFactor: 5 });
      manager.setDeformation(null);
      expect(manager.getDeformedOverlays().size).toBe(0);
      // Now unhide — deformation is off, so no overlay should appear.
      manager.setVisibility('A', 'show');
      expect(manager.getDeformedOverlays().size).toBe(0);
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
    });
  });

  describe('setDeformation — sync re-apply', () => {
    // vertA1/dispA1 kept for test (c) which sets deformation BEFORE syncing entity C
    const vertA1 = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const dispA1 = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);
    // New data with different vertices/displaced
    const vertA2 = new Float32Array([0, 0, 0, 2, 0, 0, 0, 2, 0]);
    const dispA2 = new Float32Array([0.5, 0, 0, 2.5, 0, 0, 0.5, 2, 0]);

    it('(a) setDeformation({warpFactor:5}) then sync with new data → positions reflect new data blended at W=5', () => {
      const { manager } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 5 });
      vi.clearAllMocks();

      // Sync with new data for same entity
      manager.sync({ A: { entity_path: 'A', vertices: vertA2.slice(), indices: new Uint32Array([0, 1, 2]), normals: null, displaced_positions: dispA2.slice() } });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      const expectedA2W5 = new Float32Array(vertA2.length);
      for (let i = 0; i < vertA2.length; i++) {
        expectedA2W5[i] = vertA2[i] + 5 * (dispA2[i] - vertA2[i]);
      }
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(expectedA2W5));
    });

    it('(b) sync mesh before setDeformation, then setDeformation applies to existing mesh', () => {
      const { manager, vertices, displaced } = setupDeformableMesh();

      // Call setDeformation AFTER sync — must apply to existing mesh
      manager.setDeformation({ warpFactor: 3 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      const expectedA1W3 = new Float32Array(vertices.length);
      for (let i = 0; i < vertices.length; i++) {
        expectedA1W3[i] = vertices[i] + 3 * (displaced[i] - vertices[i]);
      }
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(expectedA1W3));
    });

    // --- Amendment: suggestion 1 ---
    it('(c) mesh synced AFTER setDeformation({warpFactor:5}) immediately gets an undeformed overlay', () => {
      // Set deformation first, then introduce a brand-new entity via sync.
      // The new mesh should receive both the warp AND an undeformed overlay —
      // visually symmetric with meshes that were present at toggle time.
      const scene = new Scene();
      const manager = createMeshManager(scene);
      manager.setDeformation({ warpFactor: 5 });
      vi.clearAllMocks();

      // Sync a new entity with displaced_positions AFTER setDeformation.
      manager.sync({ C: { entity_path: 'C', vertices: vertA1.slice(), indices: new Uint32Array([0, 1, 2]), normals: null, displaced_positions: dispA1.slice() } });

      // The new entity must have an overlay.
      expect(manager.getDeformedOverlays().has('C')).toBe(true);
      expect(manager.getDeformedOverlays().size).toBe(1);

      // The overlay's position attribute must equal the original (un-warped) vertices.
      const overlay = manager.getDeformedOverlays().get('C')!;
      const posAttr = (overlay.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertA1));
    });

    // --- Amendment: suggestion 2a ---
    it('(d) sync with new vertices while deformation active rebuilds overlay with fresh original vertices', () => {
      // When a mesh's vertices change while deformation is active, the existing overlay
      // still holds the OLD Float32Array. The implementation must rebuild the overlay
      // pointing at the freshly-cached meshOriginalVertices entry.
      const { manager } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 5 });
      vi.clearAllMocks();

      // Re-sync with brand-new vertices and displaced_positions.
      manager.sync({ A: { entity_path: 'A', vertices: vertA2.slice(), indices: new Uint32Array([0, 1, 2]), normals: null, displaced_positions: dispA2.slice() } });

      // Overlay must still exist.
      expect(manager.getDeformedOverlays().has('A')).toBe(true);

      // The overlay's position.array must equal the NEW original vertices (vertA2), not vertA1.
      const overlay = manager.getDeformedOverlays().get('A')!;
      const posAttr = (overlay.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertA2));
    });

    // --- Amendment: suggestion 2b ---
    it('(e) sync removes displaced_positions while deformation active → overlay torn down', () => {
      // If a backend re-sync drops displaced_positions (e.g. FEA solve was removed),
      // the existing overlay must be removed so no ghost shape lingers.
      const { manager } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 5 });

      const overlayBefore = manager.getDeformedOverlays().get('A')!;
      const overlayGeom = overlayBefore.geometry;
      vi.clearAllMocks();

      // Re-sync WITHOUT displaced_positions.
      manager.sync({ A: { entity_path: 'A', vertices: vertA2.slice(), indices: new Uint32Array([0, 1, 2]), normals: null } });

      // Overlay must have been removed.
      expect(manager.getDeformedOverlays().has('A')).toBe(false);
      expect(manager.getDeformedOverlays().size).toBe(0);
      // Overlay geometry must have been disposed.
      expect(overlayGeom.dispose).toHaveBeenCalledOnce();
    });
  });

  describe('setDeformation — mixed mesh (with and without displaced_positions)', () => {
    const vertB = new Float32Array([2, 0, 0, 3, 0, 0, 2, 1, 0]);

    function setupMixedMeshes() {
      return setupDeformableMesh({
        B: {
          entity_path: 'B',
          vertices: vertB.slice(),
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        },
      });
    }

    it('setDeformation({warpFactor:5}): mesh A warped, mesh B unchanged; both still in getSceneMeshes', () => {
      const { manager, vertices, displaced } = setupMixedMeshes();
      manager.setDeformation({ warpFactor: 5 });

      const meshA = manager.getSceneMeshes().get('A')!;
      const meshB = manager.getSceneMeshes().get('B')!;

      // Mesh A: position should be warped
      const posA = (meshA.geometry as any).attributes.position;
      const expectedA = new Float32Array(vertices.length);
      for (let i = 0; i < vertices.length; i++) {
        expectedA[i] = vertices[i] + 5 * (displaced[i] - vertices[i]);
      }
      expect(Array.from(posA.array as Float32Array)).toEqual(Array.from(expectedA));

      // Mesh B: position should be unchanged (deep-equals its original vertices)
      const posB = (meshB.geometry as any).attributes.position;
      expect(Array.from(posB.array as Float32Array)).toEqual(Array.from(vertB));

      // Both still in scene
      expect(manager.getSceneMeshes().size).toBe(2);
    });

    it('setDeformation(null) after mixed: mesh A restored, mesh B still unchanged', () => {
      const { manager, vertices } = setupMixedMeshes();
      manager.setDeformation({ warpFactor: 5 });
      manager.setDeformation(null);

      const meshA = manager.getSceneMeshes().get('A')!;
      const meshB = manager.getSceneMeshes().get('B')!;

      const posA = (meshA.geometry as any).attributes.position;
      expect(Array.from(posA.array as Float32Array)).toEqual(Array.from(vertices));

      const posB = (meshB.geometry as any).attributes.position;
      expect(Array.from(posB.array as Float32Array)).toEqual(Array.from(vertB));
    });
  });

  describe('setDeformation — linear blend', () => {
    it('(a) setDeformation({warpFactor:1}) sets position.array to displaced_positions and needsUpdate=true', () => {
      const { manager, displaced } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 1 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(displaced));
      expect(posAttr.needsUpdate).toBe(true);
    });

    it('(b) setDeformation({warpFactor:10}) extrapolates position.array', () => {
      const { manager, vertices, displaced } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 10 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      // Compute expected via the same float32 reference formula so float32 rounding
      // is accounted for (e.g. 1 + 10*(1.1_f32 - 1) may not be exactly 2.0 in float64).
      const expectedW10 = new Float32Array(vertices.length);
      for (let i = 0; i < vertices.length; i++) {
        expectedW10[i] = vertices[i] + 10 * (displaced[i] - vertices[i]);
      }
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(expectedW10));
      // Spot-check the algebraic values are correct (~1.0 and ~2.0 as expected).
      expect(posAttr.array[0]).toBeCloseTo(1.0, 3); // 0 + 10*(0.1-0)
      expect(posAttr.array[3]).toBeCloseTo(2.0, 3); // 1 + 10*(1.1-1)
      expect(posAttr.needsUpdate).toBe(true);
    });

    it('(c) setDeformation({warpFactor:0}) restores position.array to original vertices', () => {
      const { manager, vertices } = setupDeformableMesh();
      manager.setDeformation({ warpFactor: 0 });

      const mesh = manager.getSceneMeshes().get('A')!;
      const posAttr = (mesh.geometry as any).attributes.position;
      expect(Array.from(posAttr.array as Float32Array)).toEqual(Array.from(vertices));
      expect(posAttr.needsUpdate).toBe(true);
    });
  });

  describe('input MeshData.vertices is not mutated by setDeformation warp', () => {
    it('(a) createMeshFromData path: input buffer is not mutated by warp', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      const callerVerts = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      const displaced = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);
      const snapshot = Array.from(callerVerts);

      const meshData: MeshData = {
        entity_path: 'A',
        vertices: callerVerts,
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        displaced_positions: displaced,
      };

      manager.sync({ A: meshData });
      manager.setDeformation({ warpFactor: 2 });

      // The caller's original Float32Array must be untouched — the warp writes
      // into the position buffer which must be a copy, not the input reference.
      expect(Array.from(callerVerts)).toEqual(snapshot);

      // Also verify the warp actually ran — if applyWarpToMesh silently no-op'd
      // (e.g. a future regression in the side-table lookup) the snapshot equality
      // above would still pass while failing to exercise the warp path.
      // callerVerts: [0,0,0, 1,0,0, 0,1,0], displaced: [0.1,0,0, 1.1,0,0, 0.1,1,0]
      // vertex 0 x: 0 + 2*(0.1-0) = 0.2
      const mesh = manager.getSceneMeshes().get('A')!;
      const posArr = mesh.geometry.attributes.position.array as Float32Array;
      expect(Array.from(posArr)).not.toEqual(snapshot);
      expect(posArr[0]).toBeCloseTo(0.2);
    });

    it('(b) updateMeshGeometry same-length path — caller buffer is not written by warp', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Initial sync (3 vertices = 9 floats) to get the mesh created.
      const initialVerts = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      const displaced1 = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);
      manager.sync({
        A: {
          entity_path: 'A',
          vertices: initialVerts,
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
          displaced_positions: displaced1,
        },
      });

      // Re-sync with SAME length (9 floats) — hits the same-length branch
      // (posAttr.array = data.vertices) in updateMeshGeometry.
      const callerVerts2 = new Float32Array([0.5, 0, 0, 1.5, 0, 0, 0.5, 1, 0]);
      const displaced2 = new Float32Array([0.6, 0, 0, 1.6, 0, 0, 0.6, 1, 0]);
      const snapshot = Array.from(callerVerts2);
      manager.sync({
        A: {
          entity_path: 'A',
          vertices: callerVerts2,
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
          displaced_positions: displaced2,
        },
      });

      // Trigger warp — without the fix, posAttr.array === callerVerts2 and
      // applyWarpToMesh writes into callerVerts2.
      manager.setDeformation({ warpFactor: 2 });

      expect(Array.from(callerVerts2)).toEqual(snapshot);

      // Also verify warp actually ran on the (copied) position buffer.
      // callerVerts2: [0.5,0,0, 1.5,0,0, 0.5,1,0], displaced2: [0.6,0,0, ...]
      // vertex 0 x: 0.5 + 2*(0.6-0.5) = 0.7
      const mesh = manager.getSceneMeshes().get('A')!;
      const posArr = mesh.geometry.attributes.position.array as Float32Array;
      expect(Array.from(posArr)).not.toEqual(snapshot);
      expect(posArr[0]).toBeCloseTo(0.7);
    });

    it('(c) updateMeshGeometry different-length path — caller buffer is not written by warp', () => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      // Initial sync with 3 vertices (9 floats).
      const initialVerts = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      const initialDisplaced = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0]);
      manager.sync({
        A: {
          entity_path: 'A',
          vertices: initialVerts,
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
          displaced_positions: initialDisplaced,
        },
      });

      // Re-sync with DIFFERENT length (4 vertices = 12 floats) — hits the
      // different-length branch (new BufferAttribute(data.vertices, 3)) in
      // updateMeshGeometry.
      const callerVerts3 = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0]);
      const displaced3 = new Float32Array([0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0, 1.1, 1, 0]);
      const snapshot = Array.from(callerVerts3);
      manager.sync({
        A: {
          entity_path: 'A',
          vertices: callerVerts3,
          indices: new Uint32Array([0, 1, 2, 2, 3, 0]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1]),
          displaced_positions: displaced3,
        },
      });

      // Trigger warp — without the fix, the new BufferAttribute aliases
      // callerVerts3 and applyWarpToMesh writes into it.
      manager.setDeformation({ warpFactor: 2 });

      expect(Array.from(callerVerts3)).toEqual(snapshot);

      // Also verify warp actually ran on the (copied) position buffer.
      // callerVerts3: [0,0,0, 1,0,0, 0,1,0, 1,1,0], displaced3: [0.1,0,0, ...]
      // vertex 0 x: 0 + 2*(0.1-0) = 0.2
      const mesh = manager.getSceneMeshes().get('A')!;
      const posArr = mesh.geometry.attributes.position.array as Float32Array;
      expect(Array.from(posArr)).not.toEqual(snapshot);
      expect(posArr[0]).toBeCloseTo(0.2);
    });
  });
});

// ---------------------------------------------------------------------------
// T6 end-to-end acceptance: meshCount signal (step-8)
// ---------------------------------------------------------------------------
// Wire viewStateStore → meshManager and verify that:
//   (a) aux child is excluded from getSceneMeshes() (meshCount === 1) when
//       default_visible:false and the default auto:default view is active.
//   (b) toggling setVisibility on the aux path → meshCount === 2.
// This locks the full pipeline: defaultVisibilityFor → getAllEffective →
// meshManager.setVisibility → getSceneMeshes() (the bridge's meshCount source).

describe('T6 meshCount acceptance: aux excluded by default, revealed on toggle', () => {
  // Lazy import so the mock is in place when the module loads.
  let createViewStateStore: typeof import('../../stores/viewStateStore')['createViewStateStore'];
  let makeNodeHelper: typeof import('../test-utils')['makeNode'];

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGeometries.length = 0;
    mockMaterials.length = 0;
    mockMeshes.length = 0;
    mockBasicMaterials.length = 0;
    mockPhongMaterials.length = 0;
    mockGroups.length = 0;
    const vssMod = await import('../../stores/viewStateStore');
    createViewStateStore = vssMod.createViewStateStore;
    const utilsMod = await import('../test-utils');
    makeNodeHelper = utilsMod.makeNode;
  });

  it('product realization in meshCount, aux realization excluded; toggle includes it', () => {
    createRoot((dispose) => {
      const scene = new Scene();
      const manager = createMeshManager(scene);
      vi.clearAllMocks();

      const productPath = 'Asm.part#realization[0]';
      const auxPath = 'Asm.jig#realization[0]';

      // Distinct world-offset vertices to represent composed world pose.
      // product child placed at +30 mm X, aux child at +50 mm Y (represent T5 baked transforms).
      const productVerts = new Float32Array([30, 0, 0,  31, 0, 0,  30, 1, 0]);
      const auxVerts     = new Float32Array([ 0,50, 0,   1,50, 0,   0,51, 0]);

      // Sync both meshes into the manager (both arrive as 'show' initially).
      manager.sync({
        [productPath]: {
          entity_path: productPath,
          vertices: productVerts,
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        },
        [auxPath]: {
          entity_path: auxPath,
          vertices: auxVerts,
          indices: new Uint32Array([0, 1, 2]),
          normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        },
      });

      // Build the entity tree with aux flag wired through to the realization nodes.
      const tree = [
        makeNodeHelper({
          entity_path: 'Asm',
          kind: 'structure',
          children: [
            makeNodeHelper({ entity_path: 'Asm.part', kind: 'sub',
              children: [
                makeNodeHelper({ entity_path: productPath, kind: 'realization', default_visible: true }),
              ],
            }),
            makeNodeHelper({ entity_path: 'Asm.jig', kind: 'sub',
              children: [
                makeNodeHelper({ entity_path: auxPath, kind: 'realization', default_visible: false }),
              ],
            }),
          ],
        }),
      ];

      const store = createViewStateStore();
      store.regenerateAutoViews(tree);

      // Apply getAllEffective() to the manager (this is the Viewport.tsx createEffect pattern).
      const effective = store.getAllEffective();
      for (const [path, state] of Object.entries(effective)) {
        manager.setVisibility(path, state);
      }

      // ── (a) Before toggle: product visible, aux excluded from meshCount ──
      const before = manager.getSceneMeshes();
      expect(before.size).toBe(1);
      expect(before.has(productPath)).toBe(true);
      expect(before.has(auxPath)).toBe(false);

      // ── (b) Toggle: user reveals aux entity via the outline ──
      store.setVisibility(auxPath, 'show');
      // Re-apply effective map (Viewport.tsx createEffect re-runs on explicit change).
      const effectiveAfter = store.getAllEffective();
      for (const [path, state] of Object.entries(effectiveAfter)) {
        manager.setVisibility(path, state);
      }

      const after = manager.getSceneMeshes();
      expect(after.size).toBe(2);
      expect(after.has(productPath)).toBe(true);
      expect(after.has(auxPath)).toBe(true);

      dispose();
    });
  });
});
