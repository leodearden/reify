/**
 * Unit tests for surfaceManager.ts (β step-9).
 *
 * Mocks `three` (BufferGeometry, BufferAttribute, Mesh, MeshStandardMaterial, DoubleSide)
 * so tests run without a real WebGL context. Asserts the per-kind filled-surface objects
 * are added/removed/disposed correctly and that the membrane colour is distinct from the
 * strut and cable colours, the material is transparent/DoubleSide, computeVertexNormals
 * is called, and inline corner coords are packed into the geometry position buffer.
 *
 * RED until gui/src/viewport/surfaceManager.ts is implemented (step-10).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { TensegritySurfaceData } from '../../types';

// ─── Track mock instances ────────────────────────────────────────────────────

const mockMeshInstances: any[] = [];
const mockGeometryInstances: any[] = [];
const mockMaterialInstances: any[] = [];
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

// ─── Mock three ──────────────────────────────────────────────────────────────

vi.mock('three', async (importOriginal) => {
  // Only override what surfaceManager needs; spread the rest.
  const actual = await importOriginal<typeof import('three')>();

  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    dispose = vi.fn();
    computeVertexNormals = vi.fn();
    setAttribute(name: string, attr: any) {
      this.attributes[name] = attr;
    }
    getAttribute(name: string) {
      return this.attributes[name];
    }
    constructor() {
      mockGeometryInstances.push(this);
    }
  }

  class MockBufferAttribute {
    array: Float32Array;
    itemSize: number;
    constructor(array: Float32Array, itemSize: number) {
      this.array = array;
      this.itemSize = itemSize;
    }
  }

  class MockMeshStandardMaterial {
    color: string | undefined;
    transparent: boolean | undefined;
    opacity: number | undefined;
    side: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      this.transparent = opts?.transparent;
      this.opacity = opts?.opacity;
      this.side = opts?.side;
      mockMaterialInstances.push(this);
    }
  }

  class MockMesh {
    geometry: any;
    material: any;
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
      mockMeshInstances.push(this);
    }
  }

  return {
    ...actual,
    BufferGeometry: MockBufferGeometry,
    BufferAttribute: MockBufferAttribute,
    MeshStandardMaterial: MockMeshStandardMaterial,
    Mesh: MockMesh,
    DoubleSide: 2, // Three.js DoubleSide constant
  };
});

// ─── Test helpers ────────────────────────────────────────────────────────────

const mockScene = {
  add: mockSceneAdd,
  remove: mockSceneRemove,
} as any;

function makeMembraneFacet(overrides?: Partial<TensegritySurfaceData>): TensegritySurfaceData {
  return {
    entity_path: 'Patch',
    kind: 'membrane',
    i0: 0, i1: 1, i2: 2,
    x0: 0.0, y0: 0.0, z0: 0.0,
    x1: 1.0, y1: 0.0, z1: 0.0,
    x2: 0.5, y2: 0.866, z2: 0.0,
    ...overrides,
  };
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('surfaceManager', () => {
  beforeEach(() => {
    mockMeshInstances.length = 0;
    mockGeometryInstances.length = 0;
    mockMaterialInstances.length = 0;
    mockSceneAdd.mockClear();
    mockSceneRemove.mockClear();
  });

  it('sync([membraneFacet]) adds a Mesh to the scene', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);

    expect(mockSceneAdd).toHaveBeenCalled();
    expect(mockMeshInstances.length).toBeGreaterThanOrEqual(1);
  });

  it('material has transparent=true, opacity < 1, side=DoubleSide (filled translucent shaded surface)', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);

    expect(mockMaterialInstances.length).toBeGreaterThanOrEqual(1);
    const mat = mockMaterialInstances[0];
    expect(mat.transparent).toBe(true);
    expect(mat.opacity).toBeLessThan(1);
    expect(mat.opacity).toBeGreaterThan(0);
    expect(mat.side).toBe(2); // DoubleSide
  });

  it('membrane colour is distinct from strut colour #f38ba8 AND cable colour #89b4fa', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);

    expect(mockMaterialInstances.length).toBeGreaterThanOrEqual(1);
    const mat = mockMaterialInstances[0];
    expect(mat.color).toBeTruthy();
    expect(mat.color).not.toBe('#f38ba8'); // not strut red
    expect(mat.color).not.toBe('#89b4fa'); // not cable blue
  });

  it('geometry position buffer contains all 9 inline corner coords (x0,y0,z0..x2,y2,z2)', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    const facet = makeMembraneFacet();
    sm.sync([facet]);

    expect(mockGeometryInstances.length).toBeGreaterThanOrEqual(1);
    const geom = mockGeometryInstances[0];
    const posAttr = geom.getAttribute('position');
    expect(posAttr).toBeDefined();
    // 9 floats: x0,y0,z0, x1,y1,z1, x2,y2,z2
    const arr = Array.from(posAttr.array as Float32Array);
    expect(arr).toContain(facet.x0);
    expect(arr).toContain(facet.y0);
    expect(arr).toContain(facet.z0);
    expect(arr).toContain(facet.x1);
    expect(arr).toContain(facet.y1);
    expect(arr).toContain(facet.z1);
    expect(arr).toContain(facet.x2);
    expect(arr).toContain(facet.y2);
    expect(arr).toContain(facet.z2);
  });

  it('computeVertexNormals() is called on the geometry', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);

    expect(mockGeometryInstances.length).toBeGreaterThanOrEqual(1);
    expect(mockGeometryInstances[0].computeVertexNormals).toHaveBeenCalled();
  });

  it('sync([]) removes previously-added Mesh objects from the scene', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);
    mockSceneRemove.mockClear();

    sm.sync([]);

    expect(mockSceneRemove).toHaveBeenCalled();
  });

  it('dispose() removes all objects and calls geometry.dispose() + material.dispose()', async () => {
    const { createSurfaceManager } = await import('../../viewport/surfaceManager');
    const sm = createSurfaceManager(mockScene);
    sm.sync([makeMembraneFacet()]);
    mockSceneRemove.mockClear();

    sm.dispose();

    expect(mockSceneRemove).toHaveBeenCalled();
    for (const g of mockGeometryInstances) {
      expect(g.dispose).toHaveBeenCalled();
    }
    for (const m of mockMaterialInstances) {
      expect(m.dispose).toHaveBeenCalled();
    }
  });
});
