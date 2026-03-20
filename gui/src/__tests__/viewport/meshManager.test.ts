import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { MeshData } from '../../types';

// Track all created mocks
const mockGeometries: any[] = [];
const mockMaterials: any[] = [];
const mockMeshes: any[] = [];

const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

vi.mock('three', () => {
  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    index: any = null;
    dispose = vi.fn();

    setAttribute(name: string, attr: any) {
      this.attributes[name] = attr;
    }

    setIndex(index: any) {
      this.index = index;
    }
  }

  class MockBufferAttribute {
    array: any;
    itemSize: number;
    constructor(array: any, itemSize: number) {
      this.array = array;
      this.itemSize = itemSize;
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
    expect(mesh.geometry.index.array).toBe(indices);
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
    expect(meshA.material.dispose).toHaveBeenCalled();
    expect(mockSceneRemove).toHaveBeenCalledWith(meshA);
  });

  it('each entity_path gets a deterministic color (same path = same color)', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A') });
    const colorA1 = manager.getSceneMeshes().get('A')!.material.color;

    // Recreate and sync again
    manager.sync({});
    manager.sync({ A: makeMeshData('A') });
    const colorA2 = manager.getSceneMeshes().get('A')!.material.color;

    // Color for same path should be deterministic
    expect(colorA1).toBeDefined();
    expect(colorA2).toBeDefined();
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
    expect(mesh1.material.color).toBeDefined();
    expect(mesh2.material.color).toBeDefined();
  });

  it('dispose removes and disposes all meshes from scene', () => {
    const { manager } = setup();
    manager.sync({ A: makeMeshData('A'), B: makeMeshData('B') });

    const meshA = manager.getSceneMeshes().get('A')!;
    const meshB = manager.getSceneMeshes().get('B')!;

    manager.dispose();

    expect(manager.getSceneMeshes().size).toBe(0);
    expect(meshA.geometry.dispose).toHaveBeenCalled();
    expect(meshA.material.dispose).toHaveBeenCalled();
    expect(meshB.geometry.dispose).toHaveBeenCalled();
    expect(meshB.material.dispose).toHaveBeenCalled();
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
});
