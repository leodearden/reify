/**
 * Edges-in-black overlay contract for MeshManager (alpha-spine γ, task #9731).
 *
 * These tests use REAL three.js and REAL three-mesh-bvh — deliberately NO
 * `vi.mock('three')`, unlike the sibling `meshManager.test.ts`. The house mock
 * has no `EdgesGeometry`, `LineSegments` or `LineBasicMaterial`, and the
 * behaviour pinned here (which edges survive the crease threshold, where the
 * lines live in the scene graph, what gets disposed) only exists in the real
 * classes. Same opt-out precedent as `meshManager.attributeResize.test.ts`.
 *
 * Only the public seam is driven: createMeshManager, sync, setEdges,
 * getEdgesOverlays, setVisibility, setDeformation and dispose.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import {
  Scene,
  Color,
  Box3,
  Vector3,
  type BufferGeometry,
  type Group,
  type LineBasicMaterial,
  type LineSegments,
  type Object3D,
} from 'three';
import { createMeshManager, type MeshManagerContext } from '../../viewport/meshManager';
import { EDGES_THRESHOLD_ANGLE_DEG, EDGES_COLOR } from '../../viewport/edgesOverlay';
import { EDGES_OVERLAY_RENDER_ORDER } from '../../viewport/renderOrder';
import type { MeshData } from '../../types';

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/** Corner `i` of the unit cube: bit 0 = x, bit 1 = y, bit 2 = z. */
function cubeCorner(i: number, scale: number): [number, number, number] {
  return [(i & 1) * scale, ((i >> 1) & 1) * scale, ((i >> 2) & 1) * scale];
}

/** The cube's 12 triangles, wound counter-clockwise seen from outside. */
const CUBE_TRIANGLES = [
  0, 2, 3, 0, 3, 1, // -z
  4, 5, 7, 4, 7, 6, // +z
  0, 1, 5, 0, 5, 4, // -y
  2, 6, 7, 2, 7, 3, // +y
  0, 4, 6, 0, 6, 2, // -x
  1, 3, 7, 1, 7, 5, // +x
];

/** Indexed cube: 8 shared vertices, 12 triangles. */
function cube(entityPath: string, scale = 1, extra: Partial<MeshData> = {}): MeshData {
  const vertices = new Float32Array(24);
  for (let i = 0; i < 8; i++) vertices.set(cubeCorner(i, scale), i * 3);
  return {
    entity_path: entityPath,
    vertices,
    indices: new Uint32Array(CUBE_TRIANGLES),
    normals: null,
    ...extra,
  };
}

/** The same cube as a triangle soup: 36 unshared vertices, so a different vertex count. */
function cubeSoup(entityPath: string, scale: number): MeshData {
  const vertices = new Float32Array(CUBE_TRIANGLES.length * 3);
  CUBE_TRIANGLES.forEach((corner, i) => vertices.set(cubeCorner(corner, scale), i * 3));
  const indices = new Uint32Array(CUBE_TRIANGLES.length);
  indices.forEach((_, i) => (indices[i] = i));
  return { entity_path: entityPath, vertices, indices, normals: null };
}

/**
 * Two triangles sharing the edge (0,0,0)–(1,0,0), whose face normals differ by
 * `foldDeg` degrees: four boundary edges plus one shared crease.
 */
function fold(entityPath: string, foldDeg: number): MeshData {
  const phi = (foldDeg * Math.PI) / 180;
  return {
    entity_path: entityPath,
    vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0.5, 1, 0, 0.5, -Math.cos(phi), Math.sin(phi)]),
    indices: new Uint32Array([0, 1, 2, 1, 0, 3]),
    normals: null,
  };
}

// ---------------------------------------------------------------------------
// Scene-graph probes
// ---------------------------------------------------------------------------

/** The edges overlay's own Group, present in the scene only while edges are on. */
function edgesGroupIn(scene: Scene): Object3D | undefined {
  return scene.getObjectByName('edgesOverlayGroup');
}

function edgesObjectsIn(scene: Scene): Object3D[] {
  const found: Object3D[] = [];
  scene.traverse((obj) => {
    if (obj.name.startsWith('edges:')) found.push(obj);
  });
  return found;
}

function isInScene(obj: Object3D, scene: Scene): boolean {
  let node: Object3D | null = obj;
  while (node) {
    if (node === scene) return true;
    node = node.parent;
  }
  return false;
}

function segmentCount(line: LineSegments): number {
  return (line.geometry as BufferGeometry).getAttribute('position').count / 2;
}

function boxOf(geometry: BufferGeometry): Box3 {
  geometry.computeBoundingBox();
  return geometry.boundingBox!.clone();
}

function boxOfPositions(positions: Float32Array): Box3 {
  const box = new Box3();
  for (let i = 0; i < positions.length; i += 3) {
    box.expandByPoint(new Vector3(positions[i], positions[i + 1], positions[i + 2]));
  }
  return box;
}

function onDispose(target: { addEventListener: (type: 'dispose', cb: () => void) => void }): () => boolean {
  let fired = false;
  target.addEventListener('dispose', () => {
    fired = true;
  });
  return () => fired;
}

// ---------------------------------------------------------------------------

describe('meshManager edges-in-black overlay', () => {
  let scene: Scene;
  let mm: MeshManagerContext;

  beforeEach(() => {
    scene = new Scene();
    mm = createMeshManager(scene);
  });

  it('is off by default: no overlay and no edges object in the scene', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });

    expect(mm.getEdgesOverlays().size).toBe(0);
    expect(edgesObjectsIn(scene)).toEqual([]);
    expect(edgesGroupIn(scene)).toBeUndefined();
  });

  it('setEdges(true) draws exactly one black LineSegments per mesh on the edges tier, outside the mesh', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });

    mm.setEdges(true);

    const overlays = mm.getEdgesOverlays();
    expect([...overlays.keys()].sort()).toEqual(['A', 'B']);
    for (const [path, line] of overlays) {
      expect(line.isLineSegments).toBe(true);
      expect(isInScene(line, scene)).toBe(true);
      expect((line.material as LineBasicMaterial).color.equals(new Color(EDGES_COLOR))).toBe(true);
      expect(line.renderOrder).toBe(EDGES_OVERLAY_RENDER_ORDER);
      // three sorts a Group's renderOrder (as its children's groupOrder) ahead of
      // their own renderOrder, so a tiered Group would draw every edge over the
      // selection outline, which is added straight to the scene at groupOrder 0.
      const group = line.parent as Group;
      expect(group.isGroup).toBe(true);
      expect(group.renderOrder).toBe(0);
      expect(group.parent).toBe(scene);

      // Raycast safety: the three pick sites intersect the meshes recursively,
      // so an overlay parented under its mesh would be hit as that mesh.
      const mesh = mm.getSceneMeshes().get(path)!;
      expect(line.parent).not.toBe(mesh);
      expect(mesh.children.some((c) => (c as LineSegments).isLineSegments)).toBe(false);
    }
    expect(edgesObjectsIn(scene)).toHaveLength(2);
  });

  it('follows the mesh set: a mesh synced after enable gains an overlay, a dropped mesh loses its overlay', () => {
    mm.sync({ A: cube('A') });
    mm.setEdges(true);

    mm.sync({ A: cube('A'), B: cube('B', 2) });
    expect([...mm.getEdgesOverlays().keys()].sort()).toEqual(['A', 'B']);

    const lineB = mm.getEdgesOverlays().get('B')!;
    const lineBDisposed = onDispose(lineB.geometry);
    mm.sync({ A: cube('A') });

    expect([...mm.getEdgesOverlays().keys()]).toEqual(['A']);
    expect(isInScene(lineB, scene)).toBe(false);
    expect(lineBDisposed()).toBe(true);
  });

  it('replaces the overlay when a mesh is re-synced with new geometry', () => {
    mm.sync({ A: cube('A') });
    mm.setEdges(true);
    const before = mm.getEdgesOverlays().get('A')!;
    const beforeDisposed = onDispose(before.geometry);

    mm.sync({ A: cubeSoup('A', 2) });

    const overlays = mm.getEdgesOverlays();
    expect(overlays.size).toBe(1);
    const after = overlays.get('A')!;
    expect(after).not.toBe(before);
    expect(isInScene(before, scene)).toBe(false);
    expect(beforeDisposed()).toBe(true);
    const meshA = mm.getSceneMeshes().get('A')!;
    expect(boxOf(after.geometry as BufferGeometry).equals(boxOf(meshA.geometry as BufferGeometry))).toBe(true);
  });

  it('setEdges(true) twice keeps the same overlay objects', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });
    mm.setEdges(true);
    const first = mm.getEdgesOverlays();

    mm.setEdges(true);

    const second = mm.getEdgesOverlays();
    expect(second.get('A')).toBe(first.get('A'));
    expect(second.get('B')).toBe(first.get('B'));
  });

  it('setEdges(false) removes every overlay and its group, and disposes the shared material', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });
    mm.setEdges(true);
    const materialDisposed = onDispose(mm.getEdgesOverlays().get('A')!.material as LineBasicMaterial);

    mm.setEdges(false);

    expect(mm.getEdgesOverlays().size).toBe(0);
    expect(edgesObjectsIn(scene)).toEqual([]);
    expect(edgesGroupIn(scene)).toBeUndefined();
    expect(materialDisposed()).toBe(true);
  });

  it('keeps an overlay while a sync hands back the same geometry arrays, even in a new MeshData', () => {
    const a = cube('A');
    mm.sync({ A: a, B: cube('B', 2) });
    mm.setEdges(true);
    const before = mm.getEdgesOverlays();

    // An eval that touched only B: A's arrays are unchanged, B's are new.
    mm.sync({ A: { ...a }, B: cube('B', 3) });

    const after = mm.getEdgesOverlays();
    expect(after.get('A')).toBe(before.get('A'));
    expect(after.get('B')).not.toBe(before.get('B'));
  });

  it('rebuilds when new arrays arrive on the same MeshData object', () => {
    // The engine store merges an incremental mesh update into the entity's
    // existing MeshData, so changed geometry arrives on the same object.
    const a = cube('A');
    mm.sync({ A: a });
    mm.setEdges(true);
    const before = mm.getEdgesOverlays().get('A')!;

    Object.assign(a, cubeSoup('A', 2));
    mm.sync({ A: a });

    const after = mm.getEdgesOverlays().get('A')!;
    expect(after).not.toBe(before);
    expect(boxOf(after.geometry as BufferGeometry).equals(boxOfPositions(a.vertices))).toBe(true);
  });

  it('follows new displaced positions on the same vertices while deformed', () => {
    const base = cube('A');
    const shifted = (dz: number) => base.vertices.map((v, i) => (i % 3 === 2 ? v + dz : v));
    mm.sync({ A: { ...base, displaced_positions: shifted(1) } });
    mm.setEdges(true);
    mm.setDeformation({ warpFactor: 1 });

    const moved = shifted(5);
    mm.sync({ A: { ...base, displaced_positions: moved } });

    const line = mm.getEdgesOverlays().get('A')!;
    expect(boxOf(line.geometry as BufferGeometry).equals(boxOfPositions(moved))).toBe(true);
  });

  it('draws no overlay for a hidden or ghosted mesh, and restores it when shown again', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });
    mm.setEdges(true);

    mm.setVisibility('A', 'hidden');
    expect(mm.getEdgesOverlays().has('A')).toBe(false);

    mm.setVisibility('A', 'ghost');
    expect(mm.getEdgesOverlays().has('A')).toBe(false);

    mm.setVisibility('A', 'show');
    expect(mm.getEdgesOverlays().has('A')).toBe(true);
    expect(edgesObjectsIn(scene).filter((o) => o.name === 'edges:A')).toHaveLength(1);
  });

  it('enabling with a mesh already hidden or ghosted draws no overlay for it', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2), C: cube('C', 3) });
    mm.setVisibility('A', 'hidden');
    mm.setVisibility('B', 'ghost');

    mm.setEdges(true);

    expect([...mm.getEdgesOverlays().keys()]).toEqual(['C']);
    expect(edgesObjectsIn(scene).map((o) => o.name)).toEqual(['edges:C']);
  });

  it('follows the deformed shape and returns to the undeformed one', () => {
    const base = cube('A');
    const displaced = base.vertices.slice();
    for (let i = 0; i < displaced.length; i += 3) {
      displaced[i] *= 2;
      displaced[i + 2] += 0.25;
    }
    mm.sync({ A: { ...base, displaced_positions: displaced } });
    mm.setEdges(true);

    mm.setDeformation({ warpFactor: 1 });
    const deformed = mm.getEdgesOverlays().get('A')!;
    expect(boxOf(deformed.geometry as BufferGeometry).equals(boxOfPositions(displaced))).toBe(true);

    mm.setDeformation(null);
    const restored = mm.getEdgesOverlays().get('A')!;
    expect(boxOf(restored.geometry as BufferGeometry).equals(boxOfPositions(base.vertices))).toBe(true);
  });

  it('toggling deformation leaves the overlay of a mesh without displaced positions alone', () => {
    const base = cube('A');
    const displaced = base.vertices.map((v) => v * 2);
    mm.sync({ A: { ...base, displaced_positions: displaced }, B: cube('B', 2) });
    mm.setEdges(true);
    const lineB = mm.getEdgesOverlays().get('B');

    mm.setDeformation({ warpFactor: 1 });
    const deformedA = mm.getEdgesOverlays().get('A');
    expect(mm.getEdgesOverlays().get('B')).toBe(lineB);

    mm.setDeformation(null);
    expect(mm.getEdgesOverlays().get('B')).toBe(lineB);
    expect(mm.getEdgesOverlays().get('A')).not.toBe(deformedA);
  });

  it('draws a crease only where the faces fold by at least the threshold angle', () => {
    const steep = createMeshManager(new Scene());
    steep.sync({ F: fold('F', EDGES_THRESHOLD_ANGLE_DEG + 5) });
    steep.setEdges(true);
    const shallow = createMeshManager(new Scene());
    shallow.sync({ F: fold('F', EDGES_THRESHOLD_ANGLE_DEG - 5) });
    shallow.setEdges(true);

    // Four boundary edges always; the shared crease only above the threshold.
    expect(segmentCount(steep.getEdgesOverlays().get('F')!)).toBe(5);
    expect(segmentCount(shallow.getEdgesOverlays().get('F')!)).toBe(4);

    // A cube's 12 feature edges, and none of its 6 coplanar face diagonals.
    mm.sync({ A: cube('A') });
    mm.setEdges(true);
    const cubeLines = mm.getEdgesOverlays().get('A')!;
    expect(segmentCount(cubeLines)).toBe(12);
    expect((cubeLines.geometry as BufferGeometry).getAttribute('position').count).toBe(24);
  });

  it('dispose() with edges on leaves no edges object in the scene', () => {
    mm.sync({ A: cube('A'), B: cube('B', 2) });
    mm.setEdges(true);

    mm.dispose();

    expect(edgesObjectsIn(scene)).toEqual([]);
    expect(edgesGroupIn(scene)).toBeUndefined();
  });
});
