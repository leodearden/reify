/**
 * Tests for createDiagnosticOverlay(scene) in feaDiagnosticOverlay.ts (#2966, step-7/8).
 *
 * Tests the THREE.js overlay manager using real THREE.Scene-compatible mocks
 * (same pattern as wireManager.test.ts / surfaceManager.test.ts).
 *
 * Step-7 is RED: createDiagnosticOverlay is a stub that does nothing (step-6).
 * Step-8 (GREEN) implements the real Group/ArrowHelper/LineSegments logic.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { MeshData } from '../../types';
import type { FeaDiagnosticInfo } from '../../types';

// ─── Track mock instances ────────────────────────────────────────────────────

const mockArrowHelperInstances: any[] = [];
const mockLineSegmentsInstances: any[] = [];
const mockGroupInstances: any[] = [];
const mockGeometryInstances: any[] = [];
const mockMaterialInstances: any[] = [];
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

// ─── Mock three ──────────────────────────────────────────────────────────────

vi.mock('three', async () => {
  class MockVector3 {
    x: number; y: number; z: number;
    constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }
    set(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; return this; }
    normalize() { return this; }
    copy(v: any) { this.x = v.x; this.y = v.y; this.z = v.z; return this; }
  }

  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    dispose = vi.fn();
    setAttribute(name: string, attr: any) { this.attributes[name] = attr; }
    getAttribute(name: string) { return this.attributes[name]; }
    setFromPoints = vi.fn();
    constructor() { mockGeometryInstances.push(this); }
  }

  class MockFloat32BufferAttribute {
    array: Float32Array;
    itemSize: number;
    constructor(array: number[] | Float32Array, itemSize: number) {
      this.array = array instanceof Float32Array ? array : new Float32Array(array);
      this.itemSize = itemSize;
    }
  }

  class MockLineBasicMaterial {
    color: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color ?? 0xffffff;
      mockMaterialInstances.push(this);
    }
  }

  class MockMeshBasicMaterial {
    color: any;
    dispose = vi.fn();
    constructor(opts?: any) { this.color = opts?.color ?? 0xffffff; }
  }

  class MockArrowHelper {
    dir: any; origin: any; length: number; color: number;
    dispose = vi.fn();
    traverse = vi.fn();
    constructor(dir: any, origin: any, length: number, color: number) {
      this.dir = dir; this.origin = origin; this.length = length; this.color = color;
      mockArrowHelperInstances.push(this);
    }
  }

  class MockGroup {
    children: any[] = [];
    renderOrder = 0;
    add(...objs: any[]) { this.children.push(...objs); return this; }
    remove(...objs: any[]) {
      for (const o of objs) { const i = this.children.indexOf(o); if (i >= 0) this.children.splice(i, 1); }
      return this;
    }
    traverse(fn: (obj: any) => void) { fn(this); this.children.forEach((c) => c.traverse ? c.traverse(fn) : fn(c)); }
    constructor() { mockGroupInstances.push(this); }
  }

  class MockLineSegments {
    geometry: any; material: any;
    dispose = vi.fn();
    traverse = vi.fn();
    constructor(geometry: any, material: any) {
      this.geometry = geometry; this.material = material;
      mockLineSegmentsInstances.push(this);
    }
  }

  return {
    Vector3: MockVector3,
    BufferGeometry: MockBufferGeometry,
    Float32BufferAttribute: MockFloat32BufferAttribute,
    LineBasicMaterial: MockLineBasicMaterial,
    MeshBasicMaterial: MockMeshBasicMaterial,
    ArrowHelper: MockArrowHelper,
    Group: MockGroup,
    LineSegments: MockLineSegments,
  };
});

// ─── Mock scene ──────────────────────────────────────────────────────────────

const mockScene = { add: mockSceneAdd, remove: mockSceneRemove } as any;

// ─── Test data ───────────────────────────────────────────────────────────────

function makeMesh(vertices = [0, 0, 0, 1, 0, 0, 0, 1, 0]): MeshData {
  return {
    entity_path: 'test.body',
    vertices: new Float32Array(vertices),
    indices: new Uint32Array([0, 1, 2]),
    normals: null,
  };
}

const unconstrainedDiag: FeaDiagnosticInfo = {
  kind: 'Unconstrained',
  rigid_body_modes: ['TranslationX', 'TranslationY', 'TranslationZ', 'RotationX', 'RotationY', 'RotationZ'],
};
const problemElementsDiag: FeaDiagnosticInfo = {
  kind: 'ProblemElements',
  ids: [5, 12],
};
const unresolvedDiag: FeaDiagnosticInfo = {
  kind: 'UnresolvedSelector',
  selector_path: 'Body.fea_load',
};

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('createDiagnosticOverlay', () => {
  beforeEach(() => {
    mockArrowHelperInstances.length = 0;
    mockLineSegmentsInstances.length = 0;
    mockGroupInstances.length = 0;
    mockGeometryInstances.length = 0;
    mockMaterialInstances.length = 0;
    mockSceneAdd.mockClear();
    mockSceneRemove.mockClear();
    vi.resetModules();
  });

  it('sync([Unconstrained{6 modes}]) adds a Group to the scene with renderOrder > 0', async () => {
    // RED: stub sync() does nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([unconstrainedDiag], [makeMesh()]);

    expect(mockSceneAdd).toHaveBeenCalled();
    const addedGroup = mockSceneAdd.mock.calls[0][0];
    expect(addedGroup.renderOrder).toBeGreaterThan(0);
  });

  it('sync([Unconstrained{6 modes}]) creates exactly 6 ArrowHelpers in the Group', async () => {
    // RED: stub creates no ArrowHelpers.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([unconstrainedDiag], [makeMesh()]);

    expect(mockArrowHelperInstances).toHaveLength(6);
    const group = mockSceneAdd.mock.calls[0][0];
    expect(group.children).toHaveLength(6);
  });

  it('sync([ProblemElements{ids}]) adds a Group to the scene containing a LineSegments', async () => {
    // RED: stub sync() does nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([problemElementsDiag], [makeMesh()]);

    expect(mockSceneAdd).toHaveBeenCalled();
    expect(mockLineSegmentsInstances).toHaveLength(1);
    const group = mockSceneAdd.mock.calls[0][0];
    expect(group.children).toContain(mockLineSegmentsInstances[0]);
  });

  it('sync([ProblemElements]) uses a red LineBasicMaterial', async () => {
    // RED: stub creates no materials.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([problemElementsDiag], [makeMesh()]);

    expect(mockMaterialInstances).toHaveLength(1);
    // Red color is 0xff0000
    expect(mockMaterialInstances[0].color).toBe(0xff0000);
  });

  it('sync([Unconstrained, ProblemElements]) Group contains both arrows and LineSegments', async () => {
    // RED: stub creates nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([unconstrainedDiag, problemElementsDiag], [makeMesh()]);

    expect(mockArrowHelperInstances.length).toBeGreaterThan(0);
    expect(mockLineSegmentsInstances.length).toBeGreaterThan(0);
  });

  it('sync([]) adds nothing to the scene (empty diagnostics clears overlay)', async () => {
    // An empty diagnostics list should clear any prior overlay (not add a new Group).
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([], [makeMesh()]);

    // No arrows or line segments created; scene.add may or may not have been called
    // for a Group — but if it was, the Group has NO children.
    if (mockSceneAdd.mock.calls.length > 0) {
      const group = mockSceneAdd.mock.calls[0][0];
      expect(group.children).toHaveLength(0);
    } else {
      expect(mockArrowHelperInstances).toHaveLength(0);
      expect(mockLineSegmentsInstances).toHaveLength(0);
    }
  });

  it('sync([UnresolvedSelector]) renders NO geometry (list-only, data-deferred)', async () => {
    // UnresolvedSelector is display-only in the panel; no geometry rendered.
    // RED: stub does nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([unresolvedDiag], [makeMesh()]);

    // No arrows, no lines — UnresolvedSelector renders no geometry.
    expect(mockArrowHelperInstances).toHaveLength(0);
    expect(mockLineSegmentsInstances).toHaveLength(0);
  });

  it('a second sync() replaces (not accumulates) prior overlay objects', async () => {
    // sync() called twice: second call should remove the first Group before adding a new one.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);

    overlay.sync([unconstrainedDiag], [makeMesh()]);
    expect(mockSceneAdd.mock.calls.length).toBe(1);
    mockSceneRemove.mockClear();
    mockSceneAdd.mockClear();

    overlay.sync([unconstrainedDiag], [makeMesh()]);

    // Second sync: scene.remove must have been called to tear down the first Group,
    // and scene.add for the new Group.
    expect(mockSceneRemove).toHaveBeenCalled();
    expect(mockSceneAdd).toHaveBeenCalled();
  });

  it('dispose() removes the overlay Group from the scene', async () => {
    // RED: stub dispose() does nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([unconstrainedDiag], [makeMesh()]);
    mockSceneRemove.mockClear();

    overlay.dispose();

    expect(mockSceneRemove).toHaveBeenCalled();
  });

  it('dispose() frees geometry and material resources', async () => {
    // RED: stub dispose() does nothing.
    const { createDiagnosticOverlay } = await import('../../viewport/feaDiagnosticOverlay');
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([problemElementsDiag], [makeMesh()]);
    const geomsBefore = [...mockGeometryInstances];
    const matsBefore = [...mockMaterialInstances];

    overlay.dispose();

    for (const g of geomsBefore) {
      expect(g.dispose).toHaveBeenCalled();
    }
    for (const m of matsBefore) {
      expect(m.dispose).toHaveBeenCalled();
    }
  });
});
