import {
  BufferGeometry,
  BufferAttribute,
  Mesh,
  MeshStandardMaterial,
  MeshPhongMaterial,
  MeshBasicMaterial,
  Group,
  DoubleSide,
  Color,
  type Scene,
} from 'three';
import { computeBoundsTree, disposeBoundsTree } from 'three-mesh-bvh';
import type { MeshData, VisibilityState } from '../types';
import { createGhostMaterial } from './ghostMaterial';

// Patch BufferGeometry prototype for BVH acceleration
(BufferGeometry.prototype as any).computeBoundsTree = computeBoundsTree;
(BufferGeometry.prototype as any).disposeBoundsTree = disposeBoundsTree;

/** Catppuccin accent palette for deterministic mesh coloring. */
const ACCENT_PALETTE = [
  '#89b4fa', // blue
  '#cba6f7', // mauve
  '#a6e3a1', // green
  '#fab387', // peach
  '#f38ba8', // red
  '#94e2d5', // teal
  '#f9e2af', // yellow
  '#f5c2e7', // pink
];

/** Simple string hash → palette index for deterministic color assignment. */
function hashEntityPath(path: string): number {
  let hash = 0;
  for (let i = 0; i < path.length; i++) {
    hash = ((hash << 5) - hash + path.charCodeAt(i)) | 0;
  }
  return Math.abs(hash) % ACCENT_PALETTE.length;
}

function colorForEntity(entityPath: string): Color {
  return new Color(ACCENT_PALETTE[hashEntityPath(entityPath)]);
}

/**
 * Describes how to map per-vertex scalar data to vertex colours.
 * `channel` names the key in `MeshData.scalar_channels` to read.
 * `bake(scalars)` converts the scalar Float32Array to an interleaved
 * RGB Float32Array of length vertex_count * 3 (one [R,G,B] per vertex).
 */
export interface MeshColorize {
  channel: string;
  bake: (scalars: Float32Array) => Float32Array;
}

export interface MeshManagerOptions {
  colorize?: MeshColorize;
}

export interface MeshManagerContext {
  sync: (meshes: Record<string, MeshData>) => void;
  dispose: () => void;
  getSceneMeshes: () => Map<string, Mesh>;
  setVisibility: (entityPath: string, state: VisibilityState) => void;
  getGhostMeshes: () => Map<string, Mesh>;
  setColorize: (opts: MeshColorize | null) => void;
}

/**
 * Manages Three.js Mesh objects in a scene, syncing them against a
 * Record<string, MeshData> from the engine store.
 */
function validateMeshData(data: MeshData): boolean {
  if (data.vertices.length % 3 !== 0) {
    console.warn(`Invalid mesh data: vertices.length (${data.vertices.length}) is not divisible by 3`);
    return false;
  }
  const vertexCount = data.vertices.length / 3;
  for (let i = 0; i < data.indices.length; i++) {
    if (data.indices[i] >= vertexCount) {
      console.warn(`Invalid mesh data: index ${data.indices[i]} at position ${i} >= vertex count ${vertexCount}`);
      return false;
    }
  }
  return true;
}

export function createMeshManager(scene: Scene, options?: MeshManagerOptions): MeshManagerContext {
  // Active colorize config — captured at creation time and updatable via setColorize.
  let colorize: MeshColorize | null = options?.colorize ?? null;

  // Side-table: for each entity, the scalar_channels map at creation time.
  // Kept so setColorize can re-bake without requiring a full geometry sync.
  const meshScalarChannels = new Map<string, Record<string, Float32Array>>();

  const meshMap = new Map<string, Mesh>();
  const visibilityMap = new Map<string, VisibilityState>();
  const ghostMeshMap = new Map<string, Mesh>();

  // Single shared ghost material — one material instance per manager, not per ghost clone.
  const ghostMaterial: MeshBasicMaterial = createGhostMaterial();

  // Ghost Group: all ghost clones live here so they're separate from opaque meshes.
  const ghostGroup = new Group();
  ghostGroup.name = 'ghostGroup';
  scene.add(ghostGroup);

  // Cache for getSceneMeshes() — invalidated on any visibility or sync change.
  // This avoids a new Map allocation on every pointer-move raycast call.
  let sceneMeshCache: Map<string, Mesh> | null = null;

  /**
   * Returns the scalar Float32Array for the active colorize channel if:
   *   - colorize is set, AND
   *   - the mesh data exposes that channel with at least one value.
   * Returns null otherwise.
   */
  function activeScalars(data: MeshData): Float32Array | null {
    if (!colorize) return null;
    const channel = data.scalar_channels?.[colorize.channel];
    if (!channel || channel.length === 0) return null;
    return channel;
  }

  function createMeshFromData(entityPath: string, data: MeshData): Mesh | null {
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new BufferAttribute(data.vertices, 3));
    geometry.setIndex(new BufferAttribute(data.indices, 1));
    if (data.normals) {
      geometry.setAttribute('normal', new BufferAttribute(data.normals, 3));
    } else {
      geometry.computeVertexNormals();
    }

    // If colorize is active and this mesh carries the channel, build a colour
    // BufferAttribute and use MeshPhongMaterial with vertexColors.
    const scalars = activeScalars(data);
    let material: MeshStandardMaterial | MeshPhongMaterial;
    if (scalars !== null && colorize !== null) {
      const colors = colorize.bake(scalars);
      geometry.setAttribute('color', new BufferAttribute(colors, 3));
      material = new MeshPhongMaterial({
        vertexColors: true,
        flatShading: false,
        side: DoubleSide,
      });
    } else {
      material = new MeshStandardMaterial({
        color: colorForEntity(entityPath),
        side: DoubleSide,
      });
    }

    try {
      (geometry as any).computeBoundsTree();
    } catch (err) {
      geometry.dispose();
      material.dispose();
      console.error(`Failed to build BVH for mesh '${entityPath}'`, err);
      return null;
    }

    // Store the scalar channels for later setColorize re-bake operations.
    if (data.scalar_channels) {
      meshScalarChannels.set(entityPath, data.scalar_channels);
    }

    const mesh = new Mesh(geometry, material);
    mesh.name = entityPath;
    return mesh;
  }

  function updateMeshGeometry(mesh: Mesh, data: MeshData): void {
    const geometry = mesh.geometry as BufferGeometry;

    // Reuse existing BufferAttribute objects when array length matches to avoid
    // orphaning GPU-side WebGLBuffers. When length differs, create new attribute
    // because WebGL buffers have fixed size and cannot be resized.
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (posAttr && posAttr.array.length === data.vertices.length) {
      posAttr.array = data.vertices;
      (posAttr as { count: number }).count = data.vertices.length / 3;
      posAttr.needsUpdate = true;
    } else {
      geometry.setAttribute('position', new BufferAttribute(data.vertices, 3));
    }

    const indexAttr = geometry.index;
    if (indexAttr && indexAttr.array.length === data.indices.length) {
      indexAttr.array = data.indices;
      (indexAttr as { count: number }).count = data.indices.length;
      indexAttr.needsUpdate = true;
    } else {
      geometry.setIndex(new BufferAttribute(data.indices, 1));
    }

    if (data.normals) {
      const normalAttr = geometry.getAttribute('normal') as BufferAttribute | null;
      if (normalAttr && normalAttr.array.length === data.normals.length) {
        normalAttr.array = data.normals;
        (normalAttr as { count: number }).count = data.normals.length / 3;
        normalAttr.needsUpdate = true;
      } else {
        geometry.setAttribute('normal', new BufferAttribute(data.normals, 3));
      }
    } else if (geometry.getAttribute('normal')) {
      geometry.deleteAttribute('normal');
      geometry.computeVertexNormals();
    } else {
      geometry.computeVertexNormals();
    }

    // Invalidate cached bounding volumes so updated geometry is not incorrectly culled.
    // Setting to null forces Three.js to lazily recompute on next access.
    geometry.boundingSphere = null;
    geometry.boundingBox = null;

    // Rebuild BVH for the updated geometry
    try {
      (geometry as any).computeBoundsTree();
    } catch (err) {
      console.error(`Failed to rebuild BVH for mesh '${mesh.name}'`, err);
      removeMesh(mesh.name);
    }
  }

  function addGhostClone(entityPath: string, originalMesh: Mesh): void {
    // Ghost clone shares the original's BufferGeometry (no vertex duplication).
    // Position/rotation/scale are assumed to be identity — createMeshFromData never
    // applies transforms, so ghost clones always overlap their opaque counterparts.
    // If the transform model changes in the future, copy originalMesh.position/rotation/scale here.
    const ghostClone = new Mesh(originalMesh.geometry, ghostMaterial);
    ghostClone.name = `ghost:${entityPath}`;
    ghostMeshMap.set(entityPath, ghostClone);
    ghostGroup.add(ghostClone);
  }

  function removeGhostClone(entityPath: string): void {
    const ghostClone = ghostMeshMap.get(entityPath);
    if (!ghostClone) return;
    ghostGroup.remove(ghostClone);
    ghostMeshMap.delete(entityPath);
  }

  function removeMesh(entityPath: string): void {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return;

    const state = visibilityMap.get(entityPath) ?? 'show';

    // Remove from scene only if mesh is currently shown there
    if (state === 'show') {
      scene.remove(mesh);
    }

    // removeGhostClone MUST precede geometry disposal: the ghost clone shares
    // the original mesh's BufferGeometry reference. Disposing the geometry first
    // would leave the ghost clone referencing invalid GPU buffers.
    removeGhostClone(entityPath);

    (mesh.geometry as any).disposeBoundsTree();
    (mesh.geometry as BufferGeometry).dispose();
    (mesh.material as { dispose: () => void }).dispose();
    meshMap.delete(entityPath);
    meshScalarChannels.delete(entityPath);
    visibilityMap.delete(entityPath);
  }

  /**
   * Set the visibility state for an entity.
   *
   * This may be called before the mesh has arrived (e.g. before sync() is first called for this
   * entity). In that case the state is stored in visibilityMap and will be applied when sync()
   * creates the mesh. If the entity is later removed via sync({}), removeMesh() deletes its key
   * from visibilityMap, so a subsequent setVisibility call will treat the state as if it were
   * starting fresh from 'show'.
   */
  function setVisibility(entityPath: string, state: VisibilityState): void {
    const prevState = visibilityMap.get(entityPath) ?? 'show';
    visibilityMap.set(entityPath, state);

    const mesh = meshMap.get(entityPath);
    if (!mesh) {
      // Mesh hasn't arrived yet; visibilityMap pre-set will be applied when sync() adds it.
      return;
    }

    if (prevState === state) return; // no change

    sceneMeshCache = null; // invalidate cache — scene mesh set is changing

    if (prevState === 'show') {
      if (state === 'ghost') {
        scene.remove(mesh);
        addGhostClone(entityPath, mesh);
      } else if (state === 'hidden') {
        scene.remove(mesh);
      }
    } else if (prevState === 'ghost') {
      if (state === 'show') {
        removeGhostClone(entityPath);
        scene.add(mesh);
      } else if (state === 'hidden') {
        removeGhostClone(entityPath);
      }
    } else if (prevState === 'hidden') {
      if (state === 'show') {
        scene.add(mesh);
      } else if (state === 'ghost') {
        addGhostClone(entityPath, mesh);
      }
    }
  }

  /**
   * Update the active colorize config and re-bake colour BufferAttributes in place
   * for every mesh that carries the new channel. Does NOT swap materials mid-stream —
   * the material was decided at mesh-creation time. When `opts` is null the colorize
   * state is cleared; existing colour buffers are left unchanged (material teardown
   * is out of scope for this task).
   */
  function setColorize(opts: MeshColorize | null): void {
    colorize = opts;
    if (opts === null) return;

    for (const [entityPath, mesh] of meshMap) {
      const channels = meshScalarChannels.get(entityPath);
      if (!channels) continue;
      const scalars = channels[opts.channel];
      if (!scalars || scalars.length === 0) continue;

      const geometry = mesh.geometry as BufferGeometry;
      const colorAttr = geometry.getAttribute('color') as BufferAttribute | null;
      if (!colorAttr) continue; // mesh was created without colorize; skip

      const newColors = opts.bake(scalars);
      colorAttr.array = newColors;
      (colorAttr as { count: number }).count = newColors.length / 3;
      colorAttr.needsUpdate = true;
    }
  }

  function sync(meshes: Record<string, MeshData>): void {
    sceneMeshCache = null; // invalidate cache — mesh set is changing

    // Remove meshes no longer present
    for (const key of [...meshMap.keys()]) {
      if (!(key in meshes)) {
        removeMesh(key);
      }
    }

    // Add or update meshes
    for (const [entityPath, data] of Object.entries(meshes)) {
      if (!validateMeshData(data)) continue;
      if (meshMap.has(entityPath)) {
        updateMeshGeometry(meshMap.get(entityPath)!, data);
      } else {
        const mesh = createMeshFromData(entityPath, data);
        if (mesh) {
          meshMap.set(entityPath, mesh);
          const state = visibilityMap.get(entityPath) ?? 'show';
          if (state === 'show') {
            scene.add(mesh);
          } else if (state === 'ghost') {
            addGhostClone(entityPath, mesh);
          }
          // 'hidden': don't add anywhere
        }
      }
    }

    // Prune orphan visibilityMap entries: any key not present in meshMap is a
    // stale pre-set (setVisibility was called for an entity that never arrived,
    // or arrived in a previous sync cycle but was then removed). meshMap is now
    // authoritative — orphan entries would otherwise leak and cause a future
    // arrival of the same entity to silently inherit the stale visibility state.
    //
    // COUPLED INVARIANT with Viewport.tsx: the `createEffect` that consumes
    // `props.entityVisibility` (see Viewport.tsx) re-applies setVisibility for
    // every key on each reactive render cycle. So pruning here is safe —
    // legitimate, still-visible entries are immediately re-set by the Viewport
    // effect on the next tick. Together these two pieces guarantee:
    //   (a) orphan pre-sets for never-arrived or already-removed entities cannot
    //       leak into future arrivals, and
    //   (b) current authoritative visibility is re-applied after each sync.
    // Changing either side requires revisiting the other.
    for (const key of [...visibilityMap.keys()]) {
      if (!meshMap.has(key)) {
        visibilityMap.delete(key);
      }
    }
  }

  function dispose(): void {
    for (const key of [...meshMap.keys()]) {
      removeMesh(key);
    }
    // ghostGroup was added to the scene on construction; remove it explicitly
    // so it doesn't linger as an empty Group in scene.children after dispose.
    scene.remove(ghostGroup);
    ghostMaterial.dispose();
    sceneMeshCache = null;
  }

  function getSceneMeshes(): Map<string, Mesh> {
    // Use cached result when available. The cache is invalidated by setVisibility and sync,
    // so this is always consistent with the current scene state. This avoids an O(n) allocation
    // on every pointer-move raycast call (previously this was O(1) — a direct meshMap reference).
    if (sceneMeshCache !== null) return sceneMeshCache;
    const result = new Map<string, Mesh>();
    for (const [key, mesh] of meshMap) {
      const state = visibilityMap.get(key) ?? 'show';
      if (state === 'show') {
        result.set(key, mesh);
      }
    }
    sceneMeshCache = result;
    return result;
  }

  function getGhostMeshes(): Map<string, Mesh> {
    // Return a shallow copy so callers cannot accidentally mutate internal state
    // (e.g., by calling .delete() or .clear() on the returned map).
    // getGhostMeshes is only called once per sync cycle (in adjustClipping), so
    // a cache like sceneMeshCache would add complexity with no measurable benefit.
    return new Map(ghostMeshMap);
  }

  return { sync, dispose, getSceneMeshes, setVisibility, getGhostMeshes, setColorize };
}
