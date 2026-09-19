/**
 * Index-buffer aliasing invariant for MeshManager (task #6813).
 *
 * These tests use REAL three.js and REAL three-mesh-bvh — deliberately NO
 * `vi.mock('three')` and NO `vi.mock('three-mesh-bvh')`, unlike the sibling
 * `meshManager.test.ts`.  That is essential, not incidental: the house mock
 * stubs `computeBoundsTree` to a bare `vi.fn()` in its
 * `vi.mock('three-mesh-bvh')` factory, so the BVH never runs and the defect
 * below is *structurally invisible* in the mocked suite — every existing
 * meshManager test is blind to it.  Opting out of the viewport suite's
 * mocking convention follows the precedent set by
 * `meshManager.attributeResize.test.ts`, `fitCamera.test.ts` and
 * `debugCanvasInteraction.test.ts`, each of which carries the same kind of
 * header note.
 *
 * WHAT IS BEING PINNED
 * --------------------
 * `MeshData.indices` is *aliased*, not copied, into the geometry's index
 * attribute — the `indices` doc comment on the `MeshData` interface in
 * `gui/src/types.ts` documents that as a deliberate contract.  The alias is
 * installed by `geometry.setIndex(new BufferAttribute(data.indices, 1))` in
 * `createMeshFromData` and re-installed by the `indexAttr.array = data.indices`
 * branch of `updateMeshGeometry`.  The store retains that very object, via
 * `setState('meshes', mesh.entity_path, mesh)` in engineStore's
 * `applyMeshUpdate`.
 *
 * three-mesh-bvh's README, under `MeshBVH` > `.constructor`, states verbatim:
 *
 *     NOTE: The geometry's index attribute array is modified in order to
 *     build the bounds tree unless `indirect` is set to `true`.
 *
 * and the options list directly above that note gives the default as
 * `indirect: false`.  So a plain `computeBoundsTree()` permutes the *store's*
 * `Uint32Array` in place, while every per-face side array positionally keyed
 * to it — `element_index`, `element_kind`, `region_tags` — stays in the
 * original order.  The live victim is
 * `feaDiagnosticOverlay.problemElementOutlinePositions`, which reads
 * `mesh.indices` and `mesh.element_index[f]` off the same store object (it is
 * reached from `Viewport.tsx`'s FEA diagnostics sync) and therefore outlines
 * the wrong faces for the wrong element ids.  A second victim is raycast
 * `faceIndex`, captured as `probe.face_id` and re-sampled across syncs by
 * `ProbeSystem`'s `resampleAll()`.
 *
 * WHY THE FIXTURE IS 64 INTERLEAVED TRIANGLES
 * -------------------------------------------
 * three-mesh-bvh's default `maxLeafTris` is 10: a mesh at or below that becomes
 * a single leaf, the splitter never partitions, and the index array comes back
 * unpermuted even in the mutating default configuration.  Face order must also
 * disagree with spatial order, or a mesh already laid out in split order can
 * come back unchanged by coincidence.  A minimal 2-triangle fixture — the shape
 * used by the neighbouring feaDiagnosticOverlay tests — would therefore make
 * every assertion here pass *before* the fix and certify nothing.  Hence
 * `tri(64)` with `x = (t % 2 === 0) ? t : (n - t)`.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { Scene, Mesh, Raycaster, Vector3, type BufferGeometry } from 'three';
import { acceleratedRaycast } from 'three-mesh-bvh';
import { createMeshManager, type MeshManagerContext } from '../../viewport/meshManager';
import { problemElementOutlinePositions } from '../../viewport/feaDiagnosticOverlay';
import type { MeshData } from '../../types';

// ---------------------------------------------------------------------------
// MeshData factory
// ---------------------------------------------------------------------------

/** X offset of triangle `t` in an `n`-triangle fixture — deliberately NOT `t`. */
function xOf(t: number, n: number): number {
  return t % 2 === 0 ? t : n - t;
}

/**
 * `n` independent, non-degenerate, right triangles laid out along X, with face
 * order deliberately *interleaved* against spatial order so the BVH splitter is
 * forced to reorder.  `xOf` maps 0..n-1 onto the distinct integers 0..n-1, so
 * no two triangles share an X slot and a downward ray at any face's centroid
 * hits exactly one triangle.
 *
 * Triangle `t` occupies v0 = (x, 0, 0), v1 = (x + 1, 0, 0), v2 = (x, 1, 0).
 * `element_index[f] = f`, so every face is self-identifying.
 */
function tri(n: number, entityPath = 'A'): MeshData {
  const vertices = new Float32Array(n * 9);
  const normals = new Float32Array(n * 9);
  const indices = new Uint32Array(n * 3);
  const elementIndex = new Uint32Array(n);
  for (let t = 0; t < n; t++) {
    const v = t * 9;
    const x = xOf(t, n);
    vertices[v + 0] = x;
    vertices[v + 3] = x + 1;
    vertices[v + 6] = x;
    vertices[v + 7] = 1;
    normals[v + 2] = 1;
    normals[v + 5] = 1;
    normals[v + 8] = 1;
    indices[t * 3 + 0] = t * 3 + 0;
    indices[t * 3 + 1] = t * 3 + 1;
    indices[t * 3 + 2] = t * 3 + 2;
    elementIndex[t] = t;
  }
  return {
    entity_path: entityPath,
    vertices,
    indices,
    normals,
    element_index: elementIndex,
  };
}

/** Centroid of triangle `t`'s XY footprint in an `n`-triangle fixture. */
function centroidOf(t: number, n: number): { x: number; y: number } {
  const x = xOf(t, n);
  return { x: x + 1 / 3, y: 1 / 3 };
}

/**
 * The 18 position values `problemElementOutlinePositions` emits for original
 * face `t` — three edges × two endpoints × three coords, in the emission order
 * pinned by the `outlines ONLY the matching face (18 positions)` case in
 * `feaDiagnosticOverlay.overlay.test.ts`.
 */
function expectedFaceEdges(t: number, n: number): number[] {
  const x = xOf(t, n);
  const v0 = [x, 0, 0];
  const v1 = [x + 1, 0, 0];
  const v2 = [x, 1, 0];
  return [...v0, ...v1, ...v1, ...v2, ...v2, ...v0];
}

// ---------------------------------------------------------------------------
// Non-vacuity guard
// ---------------------------------------------------------------------------

/**
 * `expect(indices).toEqual(snapshot)` is a purely *negative* oracle: it also
 * holds when nothing ever touched the buffer.  A `sync()` that bailed out early
 * — a `validateMeshData` rejection, or a BVH throw routed through `removeMesh`
 * — would turn these tests green rather than red.
 *
 * So every scenario also asserts, positively, that the mesh really reached the
 * scene with a real BVH attached.  `boundsTree` being defined is what proves
 * the genuine three-mesh-bvh ran rather than a stub — the exact failure mode
 * that would occur if someone later folded this file into the suite's
 * `vi.mock('three-mesh-bvh')` convention.
 */
function expectBuiltBvh(mm: MeshManagerContext, triangles: number, entityPath = 'A'): Mesh {
  const mesh = mm.getSceneMeshes().get(entityPath);
  expect(mesh).toBeDefined();
  const geometry = mesh!.geometry as BufferGeometry;
  expect(geometry.index).not.toBeNull();
  expect(geometry.index!.count).toBe(triangles * 3);
  expect((geometry as any).boundsTree).toBeDefined();
  return mesh!;
}

const N = 64;

// ---------------------------------------------------------------------------

describe('meshManager index-buffer aliasing (#6813)', () => {
  let scene: Scene;
  let savedRaycast: typeof Mesh.prototype.raycast;

  beforeEach(() => {
    scene = new Scene();
    // Production installs acceleratedRaycast globally as a module side effect
    // (`Mesh.prototype.raycast = acceleratedRaycast` in selection.ts, at module
    // scope). Save the prototype method so a case that swaps it can
    // put THREE's own implementation back. NEVER `delete` it — that removes
    // three's implementation permanently and poisons every later case.
    savedRaycast = Mesh.prototype.raycast;
  });

  afterEach(() => {
    Mesh.prototype.raycast = savedRaycast;
  });

  it('(a) CREATE path: sync() does not permute the caller-owned index buffer', () => {
    const mm = createMeshManager(scene);
    const data = tri(N);
    const snapshot = Array.from(data.indices);

    mm.sync({ A: data });

    expectBuiltBvh(mm, N);
    expect(Array.from(data.indices)).toEqual(snapshot);
  });

  it('(b) UPDATE path (same byte size): sync() does not permute the caller-owned index buffer', () => {
    const mm = createMeshManager(scene);
    mm.sync({ A: tri(N) });

    // Same face count => same byteLength => `updateMeshGeometry` takes the
    // `indexAttr.array = data.indices` branch, re-aliasing the NEW array into
    // the EXISTING BufferAttribute rather than calling setIndex.
    const second = tri(N);
    const snapshot = Array.from(second.indices);

    mm.sync({ A: second });

    expectBuiltBvh(mm, N);
    expect(Array.from(second.indices)).toEqual(snapshot);
  });

  it('(c) UPDATE path (different byte size): sync() does not permute the caller-owned index buffer', () => {
    const mm = createMeshManager(scene);
    mm.sync({ A: tri(N) });

    // Different face count => different byteLength => `updateMeshGeometry`
    // takes the other branch and installs a fresh BufferAttribute via
    // `geometry.setIndex(new BufferAttribute(data.indices, 1))`.
    const second = tri(N + 32);
    const snapshot = Array.from(second.indices);

    mm.sync({ A: second });

    expectBuiltBvh(mm, N + 32);
    expect(Array.from(second.indices)).toEqual(snapshot);
  });

  it('(d1) geometry.index preserves the original face order', () => {
    const mm = createMeshManager(scene);
    const data = tri(N);
    const snapshot = Array.from(data.indices);

    mm.sync({ A: data });

    const mesh = expectBuiltBvh(mm, N);
    const geometry = mesh.geometry as BufferGeometry;
    expect(Array.from(geometry.index!.array)).toEqual(snapshot);
  });

  it("(d2) THREE's own raycast reports the original face index", () => {
    // Explicitly pin THREE's own implementation for this case: another module
    // in the same file registry (or a future import of selection.ts) may have
    // swapped in acceleratedRaycast.
    Mesh.prototype.raycast = savedRaycast;

    const mm = createMeshManager(scene);
    const data = tri(N);
    mm.sync({ A: data });
    const mesh = expectBuiltBvh(mm, N);

    for (const k of [7, 23, 60]) {
      const { x, y } = centroidOf(k, N);
      const raycaster = new Raycaster(new Vector3(x, y, 10), new Vector3(0, 0, -1));
      const hits = raycaster.intersectObject(mesh, false);
      expect(hits.length).toBeGreaterThan(0);
      expect(hits[0]!.faceIndex).toBe(k);
    }
  });

  it('(d3) acceleratedRaycast reports the original face index', () => {
    // The production configuration: selection.ts installs this globally onto
    // `Mesh.prototype.raycast`, and both selection.ts and ProbeSystem.tsx set
    // `firstHitOnly = true` on their raycaster.
    Mesh.prototype.raycast = acceleratedRaycast;

    const mm = createMeshManager(scene);
    const data = tri(N);
    mm.sync({ A: data });
    const mesh = expectBuiltBvh(mm, N);

    for (const k of [7, 23, 60]) {
      const { x, y } = centroidOf(k, N);
      const raycaster = new Raycaster(new Vector3(x, y, 10), new Vector3(0, 0, -1));
      (raycaster as unknown as { firstHitOnly: boolean }).firstHitOnly = true;
      const hits = raycaster.intersectObject(mesh, false);
      expect(hits.length).toBeGreaterThan(0);
      expect(hits[0]!.faceIndex).toBe(k);
    }
  });

  it('(e) the FEA diagnostic overlay still outlines the right face after sync()', () => {
    // The live production victim: problemElementOutlinePositions reads
    // mesh.indices and mesh.element_index off the SAME store-owned object that
    // was handed to sync() — `problemElementOutlinePositions` in
    // feaDiagnosticOverlay.ts, reached from Viewport.tsx's FEA diagnostics sync.
    const mm = createMeshManager(scene);
    const data = tri(N);
    const k = 23;

    mm.sync({ A: data });
    expectBuiltBvh(mm, N);

    const positions = problemElementOutlinePositions([data], new Set([k]));
    // 1 face × 3 edges × 2 endpoints × 3 coords = 18 values.
    expect(positions).toHaveLength(18);
    expect(positions).toEqual(expectedFaceEdges(k, N));
  });
});
