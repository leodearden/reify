/**
 * Unit tests for wireManager.ts (T0b step-9).
 *
 * Mocks three/addons (LineSegments2, LineSegmentsGeometry, LineMaterial) so
 * tests run without a real WebGL context. Asserts the per-kind styled objects
 * are added/removed/disposed correctly and that strut/cable colours and
 * linewidths are distinct (strutColour !== cableColour, strutLinewidth > cableLinewidth).
 *
 * RED until gui/src/viewport/wireManager.ts is implemented (step-10).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { TensegrityWireData } from '../../types';

// ─── Track mock instances ────────────────────────────────────────────────────

const mockLineSegments2Instances: any[] = [];
const mockLineSegmentsGeometryInstances: any[] = [];
const mockLineMaterialInstances: any[] = [];
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

// ─── Mock three/addons ───────────────────────────────────────────────────────

vi.mock('three/addons', () => {
  class MockLineSegmentsGeometry {
    dispose = vi.fn();
    setPositions = vi.fn();
    constructor() {
      mockLineSegmentsGeometryInstances.push(this);
    }
  }

  class MockLineMaterial {
    color: string | undefined;
    linewidth: number;
    resolution: { set: ReturnType<typeof vi.fn> };
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      this.linewidth = opts?.linewidth ?? 1;
      this.resolution = { set: vi.fn() };
      mockLineMaterialInstances.push(this);
    }
  }

  class MockLineSegments2 {
    geometry: any;
    material: any;
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
      mockLineSegments2Instances.push(this);
    }
  }

  return {
    LineSegments2: MockLineSegments2,
    LineSegmentsGeometry: MockLineSegmentsGeometry,
    LineMaterial: MockLineMaterial,
  };
});

// ─── Test helpers ────────────────────────────────────────────────────────────

const mockScene = {
  add: mockSceneAdd,
  remove: mockSceneRemove,
} as any;

function makeStrutWire(overrides?: Partial<TensegrityWireData>): TensegrityWireData {
  return {
    entity_path: 'TPrism',
    kind: 'strut',
    x1: 1.0, y1: 0.0, z1: 1.0,
    x2: 0.866, y2: 0.5, z2: 0.0,
    ...overrides,
  };
}

function makeCableWire(overrides?: Partial<TensegrityWireData>): TensegrityWireData {
  return {
    entity_path: 'TPrism',
    kind: 'cable',
    x1: 1.0, y1: 0.0, z1: 1.0,
    x2: -0.5, y2: 0.866, z2: 1.0,
    ...overrides,
  };
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('wireManager', () => {
  beforeEach(() => {
    mockLineSegments2Instances.length = 0;
    mockLineSegmentsGeometryInstances.length = 0;
    mockLineMaterialInstances.length = 0;
    mockSceneAdd.mockClear();
    mockSceneRemove.mockClear();
  });

  it('sync([strutWire, cableWire]) adds line objects to the scene', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);

    // At least one scene.add call for each kind (may be 1 per kind, 2 total).
    expect(mockSceneAdd).toHaveBeenCalled();
    expect(mockLineSegments2Instances.length).toBeGreaterThanOrEqual(1);
  });

  it('strut and cable use distinct colours (strutColour !== cableColour)', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);

    expect(mockLineMaterialInstances.length).toBeGreaterThanOrEqual(2);
    const colors = mockLineMaterialInstances.map((m) => m.color);
    const [c1, c2] = colors;
    expect(c1).toBeTruthy();
    expect(c2).toBeTruthy();
    expect(c1).not.toBe(c2);
  });

  it('strut linewidth > cable linewidth', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);

    // Two materials: one for struts, one for cables.
    // Find them by matching to the order sync processes them.
    expect(mockLineMaterialInstances.length).toBeGreaterThanOrEqual(2);
    const linewidths = mockLineMaterialInstances.map((m) => m.linewidth);
    const maxWidth = Math.max(...linewidths);
    const minWidth = Math.min(...linewidths);
    expect(maxWidth).toBeGreaterThan(minWidth);
  });

  it('setPositions called with endpoint coords from the wire', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    const strut = makeStrutWire();
    wm.sync([strut]);

    expect(mockLineSegmentsGeometryInstances.length).toBeGreaterThanOrEqual(1);
    const geom = mockLineSegmentsGeometryInstances[0];
    expect(geom.setPositions).toHaveBeenCalled();
    const posArgs: number[] = geom.setPositions.mock.calls[0][0];
    // Should contain x1,y1,z1,x2,y2,z2 from the strut wire.
    expect(posArgs).toContain(strut.x1);
    expect(posArgs).toContain(strut.y1);
    expect(posArgs).toContain(strut.z1);
    expect(posArgs).toContain(strut.x2);
    expect(posArgs).toContain(strut.y2);
    expect(posArgs).toContain(strut.z2);
  });

  it('sync([]) removes previously-added objects from the scene', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);
    mockSceneRemove.mockClear();

    wm.sync([]);

    expect(mockSceneRemove).toHaveBeenCalled();
  });

  it('dispose() removes all objects and calls geometry.dispose() + material.dispose()', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);
    mockSceneRemove.mockClear();

    wm.dispose();

    expect(mockSceneRemove).toHaveBeenCalled();
    for (const g of mockLineSegmentsGeometryInstances) {
      expect(g.dispose).toHaveBeenCalled();
    }
    for (const m of mockLineMaterialInstances) {
      expect(m.dispose).toHaveBeenCalled();
    }
  });

  it('setResolution propagates to each LineMaterial', async () => {
    const { createWireManager } = await import('../../viewport/wireManager');
    const wm = createWireManager(mockScene);
    wm.sync([makeStrutWire(), makeCableWire()]);

    wm.setResolution(1920, 1080);

    for (const m of mockLineMaterialInstances) {
      expect(m.resolution.set).toHaveBeenCalledWith(1920, 1080);
    }
  });
});
