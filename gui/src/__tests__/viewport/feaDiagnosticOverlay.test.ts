/**
 * Tests for feaDiagnosticOverlay.ts (#2966, step-5/6/7/8).
 *
 * Pure helper tests (step-5/6): plain numbers — no WebGL or three.js types needed,
 * following the bucklingAnimator.computePointCloudBounds pattern.
 *
 * createDiagnosticOverlay tests (step-7/8): mock three.js classes to track
 * instances and assert overlay Group / ArrowHelper / LineSegments behavior without
 * a real WebGL context (mirrors surfaceManager.test.ts mock pattern).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { MeshData } from '../../types';

import {
  computeMeshesBounds,
  rigidBodyArrowSpecs,
  problemElementOutlinePositions,
  createDiagnosticOverlay,
} from '../../viewport/feaDiagnosticOverlay';

// ─── THREE.js mock instance trackers (step-7/8) ───────────────────────────────

const mockGroupInstances: any[] = [];
const mockArrowHelperInstances: any[] = [];
const mockLineSegmentsInstances: any[] = [];
const mockGeometryInstances: any[] = [];
const mockMaterialInstances: any[] = [];
const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

vi.mock('three', async (importOriginal) => {
  // Spread the real three.js so untouched exports remain functional, then
  // override only the classes that createDiagnosticOverlay constructs.
  const actual = await importOriginal<typeof import('three')>();

  class MockGroup {
    renderOrder = 0;
    children: any[] = [];
    add(child: any) {
      this.children.push(child);
      return this;
    }
    remove(child: any) {
      const i = this.children.indexOf(child);
      if (i >= 0) this.children.splice(i, 1);
      return this;
    }
    constructor() {
      mockGroupInstances.push(this);
    }
  }

  class MockArrowHelper {
    _dir: any;
    _origin: any;
    length: number;
    _color: number;
    type = 'ArrowHelper';
    dispose = vi.fn();
    constructor(dir: any, origin: any, length: number, color: number) {
      this._dir = dir;
      this._origin = origin;
      this.length = length;
      this._color = color;
      mockArrowHelperInstances.push(this);
    }
  }

  class MockVector3 {
    x: number;
    y: number;
    z: number;
    constructor(x = 0, y = 0, z = 0) {
      this.x = x;
      this.y = y;
      this.z = z;
    }
    normalize() {
      const len = Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z) || 1;
      this.x /= len;
      this.y /= len;
      this.z /= len;
      return this;
    }
    set(x: number, y: number, z: number) {
      this.x = x;
      this.y = y;
      this.z = z;
      return this;
    }
  }

  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    dispose = vi.fn();
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

  class MockLineBasicMaterial {
    color: any;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      mockMaterialInstances.push(this);
    }
  }

  class MockLineSegments {
    geometry: any;
    material: any;
    type = 'LineSegments';
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
      mockLineSegmentsInstances.push(this);
    }
  }

  return {
    ...actual,
    Group: MockGroup,
    ArrowHelper: MockArrowHelper,
    Vector3: MockVector3,
    BufferGeometry: MockBufferGeometry,
    BufferAttribute: MockBufferAttribute,
    LineBasicMaterial: MockLineBasicMaterial,
    LineSegments: MockLineSegments,
  };
});

// ─── MeshData factory ─────────────────────────────────────────────────────────

/** Build a minimal MeshData from a flat XYZ vertex array. */
function makeMesh(vertices: number[]): MeshData {
  return {
    entity_path: 'test.body',
    vertices: new Float32Array(vertices),
    indices: new Uint32Array([0, 1, 2]),
    normals: null,
  };
}

// ─── computeMeshesBounds ──────────────────────────────────────────────────────

describe('computeMeshesBounds', () => {
  it('returns center [0,0,0] and radius 0 for an empty mesh list', () => {
    const result = computeMeshesBounds([]);
    expect(result.center).toEqual([0, 0, 0]);
    expect(result.radius).toBe(0);
  });

  it('returns center and radius for a single mesh', () => {
    // Vertices: two points at (-1,0,0) and (1,0,0) → center [0,0,0], radius 1
    const mesh = makeMesh([-1, 0, 0, 1, 0, 0]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center[0]).toBeCloseTo(0);
    expect(result.center[1]).toBeCloseTo(0);
    expect(result.center[2]).toBeCloseTo(0);
    expect(result.radius).toBeCloseTo(1);
  });

  it('returns center and radius for a mesh with extent in all three axes', () => {
    // Vertices: (0,0,0) and (2,4,6) → center (1,2,3), diagonal = sqrt(4+16+36) ≈ 7.483, radius ≈ 3.742
    const mesh = makeMesh([0, 0, 0, 2, 4, 6]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center[0]).toBeCloseTo(1);
    expect(result.center[1]).toBeCloseTo(2);
    expect(result.center[2]).toBeCloseTo(3);
    expect(result.radius).toBeCloseTo(Math.sqrt(4 + 16 + 36) / 2);
  });

  it('combines bounding boxes of multiple meshes', () => {
    // Mesh A: x ∈ [-2, 0], Mesh B: x ∈ [0, 2] → combined x ∈ [-2, 2], center [0,0,0]
    const meshA = makeMesh([-2, 0, 0, 0, 0, 0]);
    const meshB = makeMesh([0, 0, 0, 2, 0, 0]);
    const result = computeMeshesBounds([meshA, meshB]);
    expect(result.center[0]).toBeCloseTo(0);
    expect(result.radius).toBeCloseTo(2);
  });

  it('returns center [0,0,0] and radius 0 for a single-point mesh', () => {
    const mesh = makeMesh([5, 5, 5]);
    const result = computeMeshesBounds([mesh]);
    expect(result.center).toEqual([5, 5, 5]);
    expect(result.radius).toBeCloseTo(0);
  });
});

// ─── rigidBodyArrowSpecs ──────────────────────────────────────────────────────

describe('rigidBodyArrowSpecs', () => {
  const center: [number, number, number] = [1, 2, 3];
  const radius = 5;

  it('returns an empty array for an empty modes list', () => {
    expect(rigidBodyArrowSpecs([], center, radius)).toEqual([]);
  });

  it('returns one spec per mode', () => {
    const specs = rigidBodyArrowSpecs(
      ['TranslationX', 'TranslationY', 'TranslationZ', 'RotationX', 'RotationY', 'RotationZ'],
      center,
      radius,
    );
    expect(specs).toHaveLength(6);
  });

  it('spec origin equals the provided center', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(specs[0].origin).toEqual(center);
  });

  it('TranslationX → dir [1,0,0], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(1);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(false);
  });

  it('TranslationY → dir [0,1,0], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationY'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(1);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(false);
  });

  it('TranslationZ → dir [0,0,1], isRotation false', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationZ'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(1);
    expect(spec.isRotation).toBe(false);
  });

  it('RotationX → dir [1,0,0], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationX'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(1);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(true);
  });

  it('RotationY → dir [0,1,0], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationY'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(1);
    expect(spec.dir[2]).toBeCloseTo(0);
    expect(spec.isRotation).toBe(true);
  });

  it('RotationZ → dir [0,0,1], isRotation true', () => {
    const [spec] = rigidBodyArrowSpecs(['RotationZ'], center, radius);
    expect(spec.dir[0]).toBeCloseTo(0);
    expect(spec.dir[1]).toBeCloseTo(0);
    expect(spec.dir[2]).toBeCloseTo(1);
    expect(spec.isRotation).toBe(true);
  });

  it('spec length is scaled from radius (positive and non-zero)', () => {
    const [spec] = rigidBodyArrowSpecs(['TranslationX'], center, radius);
    expect(spec.length).toBeGreaterThan(0);
    // Length should scale with radius — larger radius → larger arrow
    const [specSmall] = rigidBodyArrowSpecs(['TranslationX'], center, 1);
    const [specLarge] = rigidBodyArrowSpecs(['TranslationX'], center, 10);
    expect(specLarge.length).toBeGreaterThan(specSmall.length);
  });

  it('translation and rotation specs have distinct colors', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX', 'RotationX'], center, radius);
    const translationColor = specs[0].color;
    const rotationColor = specs[1].color;
    expect(translationColor).not.toBe(rotationColor);
    expect(translationColor).not.toEqual(rotationColor);
  });

  it('all translation specs share the same color', () => {
    const specs = rigidBodyArrowSpecs(['TranslationX', 'TranslationY', 'TranslationZ'], center, radius);
    expect(specs[0].color).toEqual(specs[1].color);
    expect(specs[1].color).toEqual(specs[2].color);
  });

  it('all rotation specs share the same color', () => {
    const specs = rigidBodyArrowSpecs(['RotationX', 'RotationY', 'RotationZ'], center, radius);
    expect(specs[0].color).toEqual(specs[1].color);
    expect(specs[1].color).toEqual(specs[2].color);
  });
});

// ─── createDiagnosticOverlay (step-7/8) ──────────────────────────────────────

/** All six rigid-body DOF strings. */
const ALL_SIX_MODES = [
  'TranslationX',
  'TranslationY',
  'TranslationZ',
  'RotationX',
  'RotationY',
  'RotationZ',
] as const;

/** Minimal mock scene — only add/remove are called by the overlay manager. */
const mockScene = { add: mockSceneAdd, remove: mockSceneRemove } as any;

/** A valid triangle mesh (3 vertices, indices [0,1,2]). */
function makeTriangleMesh(): MeshData {
  return {
    entity_path: 'test.body',
    vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    indices: new Uint32Array([0, 1, 2]),
    normals: null,
  };
}

describe('createDiagnosticOverlay', () => {
  const simpleMesh = [makeTriangleMesh()];

  beforeEach(() => {
    // Reset instance trackers and mock call history before each test.
    mockGroupInstances.length = 0;
    mockArrowHelperInstances.length = 0;
    mockLineSegmentsInstances.length = 0;
    mockGeometryInstances.length = 0;
    mockMaterialInstances.length = 0;
    mockSceneAdd.mockClear();
    mockSceneRemove.mockClear();
  });

  it('sync([Unconstrained{6 modes}]) adds a Group to the scene with renderOrder > 0 and 6 arrow children', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'Unconstrained', rigid_body_modes: [...ALL_SIX_MODES] }], simpleMesh);

    expect(mockSceneAdd).toHaveBeenCalledOnce();
    expect(mockGroupInstances).toHaveLength(1);
    const group = mockGroupInstances[0];
    expect(group.renderOrder).toBeGreaterThan(0);
    expect(group.children).toHaveLength(6);
    expect(mockArrowHelperInstances).toHaveLength(6);
  });

  it('sync([ProblemElements{ids}]) adds a Group containing a red LineSegments object', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'ProblemElements', ids: [5, 12] }], simpleMesh);

    expect(mockSceneAdd).toHaveBeenCalledOnce();
    expect(mockGroupInstances).toHaveLength(1);
    const group = mockGroupInstances[0];
    expect(group.children.length).toBeGreaterThanOrEqual(1);
    expect(mockLineSegmentsInstances).toHaveLength(1);
    // A material should have been created with a colour (red)
    expect(mockMaterialInstances).toHaveLength(1);
    expect(mockMaterialInstances[0].color).toBeTruthy();
  });

  it('sync([Unconstrained, ProblemElements]) adds both arrows and LineSegments to the Group', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync(
      [
        { kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] },
        { kind: 'ProblemElements', ids: [1] },
      ],
      simpleMesh,
    );

    expect(mockSceneAdd).toHaveBeenCalledOnce();
    expect(mockGroupInstances).toHaveLength(1);
    expect(mockArrowHelperInstances).toHaveLength(1);
    expect(mockLineSegmentsInstances).toHaveLength(1);
    const group = mockGroupInstances[0];
    expect(group.children).toHaveLength(2); // 1 arrow + 1 LineSegments
  });

  it('sync([]) adds NO geometry to the scene', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([], simpleMesh);

    expect(mockSceneAdd).not.toHaveBeenCalled();
    expect(mockGroupInstances).toHaveLength(0);
  });

  it('sync([UnresolvedSelector]) adds NO geometry (data-deferred to P2/#4092)', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'UnresolvedSelector', selector_path: 'body.foo' }], simpleMesh);

    expect(mockSceneAdd).not.toHaveBeenCalled();
    expect(mockGroupInstances).toHaveLength(0);
  });

  it('a second sync replaces (not accumulates) prior overlay objects', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] }], simpleMesh);
    overlay.sync([{ kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] }], simpleMesh);

    // scene.add called once per sync, scene.remove called once (removing old group)
    expect(mockSceneAdd).toHaveBeenCalledTimes(2);
    expect(mockSceneRemove).toHaveBeenCalledTimes(1);
    // Two separate ArrowHelper instances (one per sync)
    expect(mockArrowHelperInstances).toHaveLength(2);
  });

  it('dispose() removes the overlay Group from the scene and is idempotent', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] }], simpleMesh);
    mockSceneRemove.mockClear();

    overlay.dispose();
    expect(mockSceneRemove).toHaveBeenCalledOnce();

    // Second dispose is a no-op
    overlay.dispose();
    expect(mockSceneRemove).toHaveBeenCalledOnce(); // still just 1
  });

  it('dispose() after ProblemElements calls geometry.dispose() and material.dispose()', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    overlay.sync([{ kind: 'ProblemElements', ids: [1] }], simpleMesh);

    overlay.dispose();

    expect(mockGeometryInstances).toHaveLength(1);
    expect(mockMaterialInstances).toHaveLength(1);
    expect(mockGeometryInstances[0].dispose).toHaveBeenCalled();
    expect(mockMaterialInstances[0].dispose).toHaveBeenCalled();
  });

  it('dispose() on a never-synced overlay is a no-op', () => {
    const overlay = createDiagnosticOverlay(mockScene);
    expect(() => overlay.dispose()).not.toThrow();
    expect(mockSceneRemove).not.toHaveBeenCalled();
  });
});

// ─── problemElementOutlinePositions (task #4883, step-7/8) ───────────────────

describe('problemElementOutlinePositions', () => {
  /**
   * 2-triangle mesh with 6 distinct vertices:
   * - Face 0 (indices 0,1,2): vertices (0,0,0), (1,0,0), (0,1,0)
   * - Face 1 (indices 3,4,5): vertices (0,0,1), (1,0,1), (0,1,1)
   * element_index maps face 0 → element 10, face 1 → element 20.
   */
  function makeTwoTriangleMeshWithElementIndex(): MeshData {
    return {
      entity_path: 'Shell.body',
      vertices: new Float32Array([
        0, 0, 0,  // vertex 0 (face 0)
        1, 0, 0,  // vertex 1 (face 0)
        0, 1, 0,  // vertex 2 (face 0)
        0, 0, 1,  // vertex 3 (face 1)
        1, 0, 1,  // vertex 4 (face 1)
        0, 1, 1,  // vertex 5 (face 1)
      ]),
      indices: new Uint32Array([0, 1, 2, 3, 4, 5]),
      normals: null,
      element_index: new Uint32Array([10, 20]),
    };
  }

  function makeTwoTriangleMeshWithoutElementIndex(): MeshData {
    return {
      entity_path: 'Tet.body',
      vertices: new Float32Array([
        0, 0, 0,  // vertex 0
        1, 0, 0,  // vertex 1
        0, 1, 0,  // vertex 2
        0, 0, 1,  // vertex 3
        1, 0, 1,  // vertex 4
        0, 1, 1,  // vertex 5
      ]),
      indices: new Uint32Array([0, 1, 2, 3, 4, 5]),
      normals: null,
      // element_index deliberately absent
    };
  }

  it('(a) filters to only face-1 edges when element_index present and problemIds = Set([20])', () => {
    const mesh = makeTwoTriangleMeshWithElementIndex();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const positions = (problemElementOutlinePositions as any)([mesh], new Set([20]));
    // Face 0 (element 10) excluded; face 1 (element 20) included.
    // Face 1 has 3 edges × 2 endpoints × 3 coords = 18 numbers.
    expect(positions).toHaveLength(18);
    // Verify the coordinates belong to face-1 vertices (z=1 throughout)
    for (let i = 2; i < positions.length; i += 3) {
      expect(positions[i]).toBeCloseTo(1); // all z coords for face-1 verts are 1
    }
  });

  it('(b) coarse fallback: emits all faces for a mesh WITHOUT element_index even when problemIds provided', () => {
    const mesh = makeTwoTriangleMeshWithoutElementIndex();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const positions = (problemElementOutlinePositions as any)([mesh], new Set([20]));
    // Coarse fallback: all 2 faces × 3 edges × 2 pts × 3 coords = 36 numbers
    expect(positions).toHaveLength(36);
  });

  it('(c) 1-arg backward-compat: emits all faces when problemIds is undefined', () => {
    const mesh = makeTwoTriangleMeshWithElementIndex();
    const positions = problemElementOutlinePositions([mesh]);
    // All 2 faces × 3 edges × 2 pts × 3 coords = 36 numbers
    expect(positions).toHaveLength(36);
  });

  it('returns [] for an empty mesh list regardless of problemIds', () => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((problemElementOutlinePositions as any)([], new Set([5]))).toHaveLength(0);
    expect(problemElementOutlinePositions([])).toHaveLength(0);
  });
});
