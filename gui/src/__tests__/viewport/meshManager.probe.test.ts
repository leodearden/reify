/**
 * Tests for the probe-related extensions to meshManager:
 *   - computeBarycentric(entityPath, faceId, point): BarycentricUV | null
 *   - sampleProbe(entityPath, faceId, bary): ProbeSample | null
 *
 * Uses the same vi.mock('three') / vi.mock('three-mesh-bvh') inline-mock
 * pattern established in meshManager.test.ts.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { MeshData } from '../../types';

// ---------------------------------------------------------------------------
// Three.js mock (mirrors meshManager.test.ts minimal shape)
// ---------------------------------------------------------------------------

const mockBasicMaterials = vi.hoisted<any[]>(() => []);
const mockPhongMaterials = vi.hoisted<any[]>(() => []);
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
      return this.attributes[name] ?? null;
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
    constructor(color?: any) { this.value = color; }
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Triangle A=(0,0,0), B=(1,0,0), C=(0,1,0), one face index [0,1,2]. */
function makeTriangleMesh(entityPath: string, extra?: Partial<MeshData>): MeshData {
  return {
    entity_path: entityPath,
    vertices: new Float32Array([
      0, 0, 0, // A
      1, 0, 0, // B
      0, 1, 0, // C
    ]),
    indices: new Uint32Array([0, 1, 2]),
    normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    ...extra,
  };
}

function setup() {
  const scene = new Scene();
  const manager = createMeshManager(scene);
  vi.clearAllMocks();
  return { scene, manager };
}

// ---------------------------------------------------------------------------
// Step 9: computeBarycentric
// ---------------------------------------------------------------------------

describe('meshManager — computeBarycentric', () => {
  it('returns null for unknown entity', () => {
    const { manager } = setup();
    const result = manager.computeBarycentric('nope', 0, { x: 0, y: 0, z: 0 });
    expect(result).toBeNull();
  });

  it('returns null for faceId whose 3*faceId+2 exceeds the index count', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });
    // Only 1 triangle (indices length = 3), face 1 would need index[5] which is out of range
    const result = manager.computeBarycentric('T', 1, { x: 0, y: 0, z: 0 });
    expect(result).toBeNull();
  });

  it('returns [u, v, w] for a point P = u*A + v*B + w*C (weights sum to 1)', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });

    // Chosen weights: u=0.2, v=0.3, w=0.5
    // A=(0,0,0), B=(1,0,0), C=(0,1,0)
    // P = 0.2*(0,0,0) + 0.3*(1,0,0) + 0.5*(0,1,0) = (0.3, 0.5, 0)
    const point = { x: 0.3, y: 0.5, z: 0 };
    const bary = manager.computeBarycentric('T', 0, point);
    expect(bary).not.toBeNull();
    expect(bary![0]).toBeCloseTo(0.2, 4);
    expect(bary![1]).toBeCloseTo(0.3, 4);
    expect(bary![2]).toBeCloseTo(0.5, 4);
  });

  it('weights sum to 1 within tolerance', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });

    const point = { x: 0.5, y: 0.3, z: 0 };
    const bary = manager.computeBarycentric('T', 0, point);
    expect(bary).not.toBeNull();
    const sum = bary![0] + bary![1] + bary![2];
    expect(sum).toBeCloseTo(1.0, 4);
  });

  it('centroid of the triangle returns weights [1/3, 1/3, 1/3]', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });

    // Centroid: ((0+1+0)/3, (0+0+1)/3, 0) = (1/3, 1/3, 0)
    const point = { x: 1 / 3, y: 1 / 3, z: 0 };
    const bary = manager.computeBarycentric('T', 0, point);
    expect(bary).not.toBeNull();
    expect(bary![0]).toBeCloseTo(1 / 3, 4);
    expect(bary![1]).toBeCloseTo(1 / 3, 4);
    expect(bary![2]).toBeCloseTo(1 / 3, 4);
  });

  it('vertex A itself returns [1, 0, 0]', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });
    const point = { x: 0, y: 0, z: 0 };
    const bary = manager.computeBarycentric('T', 0, point);
    expect(bary).not.toBeNull();
    expect(bary![0]).toBeCloseTo(1, 4);
    expect(bary![1]).toBeCloseTo(0, 4);
    expect(bary![2]).toBeCloseTo(0, 4);
  });

  it('returns null for a degenerate triangle (two coincident vertices)', () => {
    const { manager } = setup();
    // Degenerate triangle: A and B are coincident → zero area → denom ≈ 0
    const degenerateMesh: MeshData = {
      entity_path: 'Degen',
      vertices: new Float32Array([
        0, 0, 0, // A
        0, 0, 0, // B (coincident with A → zero-area triangle)
        1, 0, 0, // C
      ]),
      indices: new Uint32Array([0, 1, 2]),
      normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    };
    manager.sync({ Degen: degenerateMesh });
    const result = manager.computeBarycentric('Degen', 0, { x: 0.5, y: 0, z: 0 });
    expect(result).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Step 11: sampleProbe
// ---------------------------------------------------------------------------

describe('meshManager — sampleProbe', () => {
  it('returns null for unknown entity (staleness signal)', () => {
    const { manager } = setup();
    const result = manager.sampleProbe('nope', 0, [0.2, 0.3, 0.5]);
    expect(result).toBeNull();
  });

  it('returns null for out-of-range face (staleness signal)', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T');
    manager.sync({ T: mesh });
    // Face 1 would need index[5] which is absent
    const result = manager.sampleProbe('T', 1, [0.2, 0.3, 0.5]);
    expect(result).toBeNull();
  });

  it('displacement is null when no displaced_positions are present', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T'); // no displaced_positions
    manager.sync({ T: mesh });
    const sample = manager.sampleProbe('T', 0, [1 / 3, 1 / 3, 1 / 3]);
    expect(sample).not.toBeNull();
    expect(sample.displacement).toBeNull();
  });

  it('displacement is exact constant delta for a uniform-displacement mesh', () => {
    const { manager } = setup();
    // Every vertex displaced by exactly [0.1, 0.2, 0.3]
    const vertices = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const displaced = new Float32Array([0.1, 0.2, 0.3, 1.1, 0.2, 0.3, 0.1, 1.2, 0.3]);
    const mesh = makeTriangleMesh('T', { vertices, displaced_positions: displaced });
    manager.sync({ T: mesh });

    // Any bary should give displacement = [0.1, 0.2, 0.3]
    const sample = manager.sampleProbe('T', 0, [0.2, 0.3, 0.5]);
    expect(sample).not.toBeNull();
    expect(sample.displacement[0]).toBeCloseTo(0.1, 4);
    expect(sample.displacement[1]).toBeCloseTo(0.2, 4);
    expect(sample.displacement[2]).toBeCloseTo(0.3, 4);
  });

  it('displacement interpolates correctly for a linear-ramp displacement', () => {
    const { manager } = setup();
    // A displaced by [1,0,0], B by [0,1,0], C by [0,0,1]
    const vertices = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    const displaced = new Float32Array([1, 0, 0, 1, 1, 0, 0, 1, 1]);
    const mesh = makeTriangleMesh('T', { vertices, displaced_positions: displaced });
    manager.sync({ T: mesh });

    // bary = [0.2, 0.3, 0.5]
    // delta_A = [1,0,0], delta_B = [0,1,0], delta_C = [0,0,1]
    // displacement = 0.2*[1,0,0] + 0.3*[0,1,0] + 0.5*[0,0,1] = [0.2, 0.3, 0.5]
    const sample = manager.sampleProbe('T', 0, [0.2, 0.3, 0.5]);
    expect(sample).not.toBeNull();
    expect(sample.displacement[0]).toBeCloseTo(0.2, 4);
    expect(sample.displacement[1]).toBeCloseTo(0.3, 4);
    expect(sample.displacement[2]).toBeCloseTo(0.5, 4);
  });

  it('vonMises is null when scalar channel is absent', () => {
    const { manager } = setup();
    const mesh = makeTriangleMesh('T'); // no scalar_channels
    manager.sync({ T: mesh });
    const sample = manager.sampleProbe('T', 0, [1 / 3, 1 / 3, 1 / 3]);
    expect(sample).not.toBeNull();
    expect(sample.vonMises).toBeNull();
    expect(sample.scalars).toEqual({});
  });

  it('vonMises is interpolated and appears in sample.scalars for uniform channel', () => {
    const { manager } = setup();
    const vonMisesValues = new Float32Array([5.0, 5.0, 5.0]); // uniform
    const mesh = makeTriangleMesh('T', { scalar_channels: { vonMises: vonMisesValues } });
    manager.sync({ T: mesh });

    const sample = manager.sampleProbe('T', 0, [0.2, 0.3, 0.5]);
    expect(sample).not.toBeNull();
    expect(sample.vonMises).toBeCloseTo(5.0, 4);
    expect(sample.scalars['vonMises']).toBeCloseTo(5.0, 4);
  });

  it('vonMises is interpolated for a linear scalar channel (within 1e-4)', () => {
    const { manager } = setup();
    // Vertex values: A=1.0, B=2.0, C=3.0
    const vonMisesValues = new Float32Array([1.0, 2.0, 3.0]);
    const mesh = makeTriangleMesh('T', { scalar_channels: { vonMises: vonMisesValues } });
    manager.sync({ T: mesh });

    // bary = [0.2, 0.3, 0.5]: 0.2*1 + 0.3*2 + 0.5*3 = 0.2 + 0.6 + 1.5 = 2.3
    const sample = manager.sampleProbe('T', 0, [0.2, 0.3, 0.5]);
    expect(sample).not.toBeNull();
    expect(sample.vonMises).toBeCloseTo(2.3, 4);
    expect(sample.scalars['vonMises']).toBeCloseTo(2.3, 4);
  });

  it('generic scalar channels other than vonMises appear in sample.scalars', () => {
    const { manager } = setup();
    const pressure = new Float32Array([10.0, 10.0, 10.0]);
    const mesh = makeTriangleMesh('T', { scalar_channels: { pressure } });
    manager.sync({ T: mesh });

    const sample = manager.sampleProbe('T', 0, [1 / 3, 1 / 3, 1 / 3]);
    expect(sample).not.toBeNull();
    expect(sample.scalars['pressure']).toBeCloseTo(10.0, 4);
    // vonMises absent → null
    expect(sample.vonMises).toBeNull();
  });

  it('vector channel is interpolated component-wise into sample.vectors', () => {
    const { manager } = setup();
    // Vertex flux vectors: A=[1,0,0], B=[0,1,0], C=[0,0,1]
    const flux = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const mesh = makeTriangleMesh('T', { vector_channels: { flux } });
    manager.sync({ T: mesh });

    // bary = [0.2, 0.3, 0.5]
    // flux = 0.2*[1,0,0] + 0.3*[0,1,0] + 0.5*[0,0,1] = [0.2, 0.3, 0.5]
    const sample = manager.sampleProbe('T', 0, [0.2, 0.3, 0.5]);
    expect(sample).not.toBeNull();
    expect(sample.vectors['flux'][0]).toBeCloseTo(0.2, 4);
    expect(sample.vectors['flux'][1]).toBeCloseTo(0.3, 4);
    expect(sample.vectors['flux'][2]).toBeCloseTo(0.5, 4);
  });

  it('vector channel side-table is cleaned up in removeMesh (via sync to empty)', () => {
    const { manager } = setup();
    const flux = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const mesh = makeTriangleMesh('T', { vector_channels: { flux } });
    manager.sync({ T: mesh });
    // Verify it exists
    expect(manager.sampleProbe('T', 0, [1 / 3, 1 / 3, 1 / 3])).not.toBeNull();
    // Sync to empty removes the mesh
    manager.sync({});
    // Now sampleProbe should return null (entity gone)
    expect(manager.sampleProbe('T', 0, [1 / 3, 1 / 3, 1 / 3])).toBeNull();
  });
});
