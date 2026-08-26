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
});
