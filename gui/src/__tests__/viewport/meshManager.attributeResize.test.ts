/**
 * BufferAttribute resize-safety invariant for MeshManager (task #6757).
 *
 * These tests use REAL three.js and REAL three-mesh-bvh — deliberately NO
 * `vi.mock('three')`, unlike the sibling `meshManager.test.ts`.  That is
 * essential, not incidental: the house mock's `MockBufferAttribute` has
 * neither a `version` field nor `byteLength` semantics, and those are exactly
 * the two things THREE's throw keys on, so the defect is structurally
 * invisible in the mocked suite.  Opting out of the viewport suite's mocking
 * convention follows the precedent set by `fitCamera.test.ts` and
 * `debugCanvasInteraction.test.ts`, each of which carries the same kind of
 * header note.
 *
 * WHAT IS BEING PINNED
 * --------------------
 * WebGL buffers have a fixed size.  `THREE.WebGLAttributes.update()`
 * (node_modules/three/src/renderers/webgl/WebGLAttributes.js:204-214) therefore
 * throws when it sees the *same* BufferAttribute object again with a bumped
 * `version` while `attribute.array.byteLength` no longer matches the byte size
 * recorded when the GPU buffer was first created:
 *
 *     const data = buffers.get( attribute );
 *     if ( data === undefined ) {
 *       buffers.set( attribute, createBuffer( attribute, bufferType ) );
 *     } else if ( data.version < attribute.version ) {
 *       if ( data.size !== attribute.array.byteLength ) {
 *         throw new Error( 'THREE.WebGLAttributes: ... Resizing buffer
 *                           attributes is not supported.' );
 *       }
 *       updateBuffer( data.buffer, attribute, bufferType );
 *       data.version = attribute.version;
 *     }
 *
 * Note that it compares **byteLength**, not element length.
 *
 * WHY THE BOOKKEEPING IS EMULATED RATHER THAN RENDERED
 * ----------------------------------------------------
 * jsdom has no WebGL context, so a real `WebGLRenderer` cannot run and the
 * genuine throw can never be observed in-suite.  The condition above depends
 * on nothing but `attribute.version` and `attribute.array.byteLength`, so
 * `renderPass()` below is a faithful six-line transcription of it.  Because it
 * walks *every* attribute of *every* geometry in the scene it also generalises:
 * a regression at any site — including the undeformed overlay, ghost clones,
 * and the BVH path — surfaces as a violation without a per-site assertion.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { Scene, Mesh, BufferAttribute, type BufferGeometry } from 'three';
import { createMeshManager } from '../../viewport/meshManager';
import type { MeshData } from '../../types';

// ---------------------------------------------------------------------------
// WebGLAttributes.update() emulation
// ---------------------------------------------------------------------------

/** Per-attribute GPU-buffer bookkeeping, mirroring WebGLAttributes' WeakMap. */
interface BufferRecord {
  version: number;
  size: number;
}

function makeRenderHarness() {
  const buffers = new Map<BufferAttribute, BufferRecord>();
  const violations: string[] = [];

  /** Transcription of WebGLAttributes.update()'s size check for one attribute. */
  function visitAttr(attr: BufferAttribute, name: string, tag: string): void {
    const rec = buffers.get(attr);
    if (rec === undefined) {
      // First sighting: THREE would call createBuffer() and record the size.
      buffers.set(attr, { version: attr.version, size: attr.array.byteLength });
      return;
    }
    if (rec.version < attr.version) {
      if (rec.size !== attr.array.byteLength) {
        violations.push(
          `${name}: recorded size ${rec.size} != current byteLength ` +
            `${attr.array.byteLength} (v${rec.version}->v${attr.version}) [pass=${tag}]`,
        );
      }
      rec.version = attr.version;
    }
  }

  /**
   * Emulate one draw frame: walk the scene and run the size check over every
   * attribute (plus the index) of every geometry encountered.
   */
  function renderPass(scene: Scene, tag: string): void {
    scene.traverse((obj) => {
      const geometry = (obj as Mesh).geometry as BufferGeometry | undefined;
      if (!geometry || !geometry.attributes) return;
      for (const [name, attr] of Object.entries(geometry.attributes)) {
        visitAttr(attr as BufferAttribute, name, tag);
      }
      if (geometry.index) visitAttr(geometry.index as BufferAttribute, 'index', tag);
    });
  }

  return { renderPass, violations };
}

// ---------------------------------------------------------------------------
// MeshData factories
// ---------------------------------------------------------------------------

/**
 * `n` independent, non-degenerate triangles laid out along +X.
 * Vertex count is `n * 3`, so re-syncing at a different `n` always changes the
 * byte size of every per-vertex attribute.
 */
function tri(n: number, extra: Partial<MeshData> = {}): MeshData {
  const vertices = new Float32Array(n * 9);
  const normals = new Float32Array(n * 9);
  const indices = new Uint32Array(n * 3);
  for (let t = 0; t < n; t++) {
    const v = t * 9;
    // v0 = (t, 0, 0), v1 = (t + 1, 0, 0), v2 = (t, 1, 0)
    vertices[v + 0] = t;
    vertices[v + 3] = t + 1;
    vertices[v + 6] = t;
    vertices[v + 7] = 1;
    normals[v + 2] = 1;
    normals[v + 5] = 1;
    normals[v + 8] = 1;
    indices[t * 3 + 0] = t * 3 + 0;
    indices[t * 3 + 1] = t * 3 + 1;
    indices[t * 3 + 2] = t * 3 + 2;
  }
  return { entity_path: 'A', vertices, indices, normals, ...extra };
}

/** `tri(n)` plus a per-vertex `vonMises` scalar channel (length `n * 3`). */
function triScalars(n: number, extra: Partial<MeshData> = {}): MeshData {
  const vonMises = new Float32Array(n * 3);
  for (let i = 0; i < vonMises.length; i++) vonMises[i] = i / vonMises.length;
  return tri(n, { scalar_channels: { vonMises }, ...extra });
}

/** `tri(n)` plus an FEA displacement field, so setDeformation has something to warp. */
function triDisplaced(n: number): MeshData {
  const base = tri(n);
  const displaced = base.vertices.slice();
  for (let i = 0; i < displaced.length; i += 3) displaced[i + 2] += 0.25;
  return { ...base, displaced_positions: displaced };
}

/**
 * Mirrors the production `bakeColours` length contract
 * (gui/src/viewport/colormap.ts:152 — `new Float32Array(scalars.length * 3)`),
 * so the baked colour array's byte size tracks vertex count exactly.
 */
const bake = (s: Float32Array): Float32Array => new Float32Array(s.length * 3).fill(0.5);

// ---------------------------------------------------------------------------

describe('meshManager BufferAttribute resize safety (#6757)', () => {
  let scene: Scene;
  let renderPass: (scene: Scene, tag: string) => void;
  let violations: string[];

  beforeEach(() => {
    scene = new Scene();
    const harness = makeRenderHarness();
    renderPass = harness.renderPass;
    violations = harness.violations;
  });

  it('re-bakes the colour buffer across a vertex-count change without resizing it', () => {
    const mm = createMeshManager(scene, { colorize: { channel: 'vonMises', bake } });

    mm.sync({ A: triScalars(2) });
    renderPass(scene, 'after-first-sync');

    mm.sync({ A: triScalars(5) });
    renderPass(scene, 'after-resync');

    expect(violations).toEqual([]);
  });

  it('re-bakes via setColorize after an off/on toggle spanning a vertex-count change', () => {
    // Site 2 (setColorize) is reachable independently of the sync() re-bake:
    // setColorize(null) deliberately leaves the colour buffer attached to the
    // geometry (see the setColorize JSDoc), so a subsequent count-changing
    // sync() skips the `if (colorize)` re-bake entirely and leaves a stale
    // 3N-float colour attribute on a geometry whose position is now 3M. Turning
    // FEA mode back on then bakes 3M floats into that stale attribute.
    //
    // This is what a user toggling FEA mode off, editing the model, and
    // toggling FEA back on produces. Viewport.tsx happens to pair
    // setColorize(null) with rebuildMaterials(), which would strip the colour
    // attribute — deliberately NOT called here, so the setColorize path is
    // isolated rather than masked.
    const mm = createMeshManager(scene, { colorize: { channel: 'vonMises', bake } });

    mm.sync({ A: triScalars(2) });
    renderPass(scene, 'after-first-sync');

    mm.setColorize(null);

    mm.sync({ A: triScalars(5) });
    renderPass(scene, 'after-resync-uncolorized');

    mm.setColorize({ channel: 'vonMises', bake });
    renderPass(scene, 'after-recolorize');

    expect(violations).toEqual([]);
  });

  // -------------------------------------------------------------------------
  // Characterization pins for the paths that were ALREADY correct.
  //
  // These were confirmed violation-free on the pre-fix tree, so they are not
  // expected to have a RED phase. Their job is to freeze the currently-correct
  // sites — position/index/normal (guarded by task 3402), the
  // computeVertexNormals branch, the deformation warp/restore/overlay paths and
  // rebuildMaterials — so a future refactor cannot silently reintroduce the
  // defect at a site this task did not touch.
  // -------------------------------------------------------------------------

  it('re-syncs position/index/normal across a vertex-count change (no colorize)', () => {
    const mm = createMeshManager(scene);

    mm.sync({ A: tri(2) });
    renderPass(scene, 'after-first-sync');

    mm.sync({ A: tri(5) });
    renderPass(scene, 'after-resync');

    expect(violations).toEqual([]);
  });

  it('re-syncs across a vertex-count change when normals are computed, not supplied', () => {
    // Exercises the deleteAttribute('normal') + computeVertexNormals() branch:
    // THREE would otherwise resize an existing wrong-sized normal attribute in
    // place rather than allocating a fresh one.
    const mm = createMeshManager(scene);

    mm.sync({ A: tri(2, { normals: null }) });
    renderPass(scene, 'after-first-sync');

    mm.sync({ A: tri(5, { normals: null }) });
    renderPass(scene, 'after-resync');

    expect(violations).toEqual([]);
  });

  it('warps, re-syncs and restores across a vertex-count change with deformation active', () => {
    // Covers applyWarpToMesh, restoreOriginalToMesh and the undeformed-overlay
    // rebuild. All three write element-wise into a fixed-size destination and
    // must never reassign `.array`; the overlay is rebuilt (not resized) on a
    // topology-changing sync.
    const mm = createMeshManager(scene);

    mm.sync({ A: triDisplaced(2) });
    renderPass(scene, 'after-first-sync');

    mm.setDeformation({ warpFactor: 1 });
    renderPass(scene, 'after-deform-on');
    // Non-vacuity: deformation really engaged (an overlay exists to rebuild).
    expect(mm.getDeformedOverlays().size).toBe(1);

    mm.sync({ A: triDisplaced(5) });
    renderPass(scene, 'after-resync');

    mm.setDeformation({ warpFactor: 2 });
    renderPass(scene, 'after-warp-change');

    mm.setDeformation(null);
    renderPass(scene, 'after-deform-off');

    expect(violations).toEqual([]);
  });

  it('re-syncs a ghosted entity across a vertex-count change', () => {
    // Ghost clones share the original's BufferGeometry object, so a resize on
    // the original is visible through the clone as well.
    const mm = createMeshManager(scene);

    mm.sync({ A: tri(2) });
    mm.setVisibility('A', 'ghost');
    renderPass(scene, 'after-first-sync');
    // Non-vacuity: a ghost clone really exists in the scene graph.
    expect(mm.getGhostMeshes().size).toBe(1);

    mm.sync({ A: tri(5) });
    renderPass(scene, 'after-resync');

    expect(violations).toEqual([]);
  });

  it('rebuilds materials after a colorize re-sync without resizing any attribute', () => {
    // rebuildMaterials must keep installing colour via setAttribute and never
    // fall back to in-place assignment.
    const mm = createMeshManager(scene, { colorize: { channel: 'vonMises', bake } });

    mm.sync({ A: triScalars(2) });
    renderPass(scene, 'after-first-sync');

    mm.sync({ A: triScalars(5) });
    renderPass(scene, 'after-resync');

    mm.rebuildMaterials();
    renderPass(scene, 'after-rebuild-materials');

    expect(violations).toEqual([]);
  });
});
