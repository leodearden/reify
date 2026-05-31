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
import type { BarycentricUV, ProbeSample } from '../stores/probeStore';

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
  /**
   * Apply or remove a deformed-shape view.
   *
   * When `opts` is set: for each mesh that has `displaced_positions`, writes
   * `position[i] = orig[i] + W * (disp[i] - orig[i])` into the position
   * BufferAttribute in-place and sets `needsUpdate = true`.  Meshes without
   * `displaced_positions` are untouched.
   *
   * When `opts` is null: restores every position buffer to the cached original
   * vertices and sets `needsUpdate = true`.  No-op if deformation was already
   * inactive.
   *
   * Also adds / removes a translucent undeformed-shape overlay per FEA mesh.
   */
  setDeformation: (opts: { warpFactor: number } | null) => void;
  /** Returns a copy of the undeformed overlay mesh map (keyed by entity path). */
  getDeformedOverlays: () => Map<string, Mesh>;
  /**
   * Rebuild mesh materials in place based on the current colorize state.
   *
   * When colorize is null: replaces each mesh's material with a fresh
   * MeshStandardMaterial (using the deterministic entity colour), removes the
   * `color` BufferAttribute from the geometry, and disposes the old material.
   * Ghost clones share geometry (not material) with their originals, so they
   * are unaffected.
   *
   * When colorize is set: ensures each mesh that has the channel uses
   * MeshPhongMaterial with a baked colour attribute; meshes that lack the
   * channel fall back to MeshStandardMaterial. This is the additive counterpart
   * to the null path — it allows a mid-stream material upgrade without a full
   * sync/re-sync cycle.
   *
   * In both branches geometry vertices/indices/normals/BVH are NOT touched.
   */
  rebuildMaterials: () => void;
  /**
   * Compute barycentric coordinates of `point` within face `faceId` of entity
   * `entityPath`, reading the LIVE geometry position attribute.
   *
   * Returns null when the entity is absent, the index buffer is missing, faceId
   * is out of range (3*faceId+2 >= index.count), or the triangle is degenerate.
   *
   * Uses the standard projected-area formula; stable across three versions.
   */
  computeBarycentric: (
    entityPath: string,
    faceId: number,
    point: { x: number; y: number; z: number },
  ) => BarycentricUV | null;
  /**
   * Sample FEA quantities at a probe point identified by barycentric coordinates.
   *
   * Returns null when the entity is absent or the faceId is out of range —
   * this is the staleness signal used by resampleAll().
   *
   * When present, interpolates:
   *   - displacement from (meshDisplacedPositions − meshOriginalVertices)
   *   - vonMises and all other scalars from meshScalarChannels
   *   - all vector channels from meshVectorChannels
   */
  sampleProbe: (
    entityPath: string,
    faceId: number,
    bary: BarycentricUV,
  ) => ProbeSample | null;
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

  // Active deformation config — null means undeformed view.
  let currentDeformation: { warpFactor: number } | null = null;

  // Side-table: for each entity, the scalar_channels map at creation time.
  // Kept so setColorize can re-bake without requiring a full geometry sync.
  const meshScalarChannels = new Map<string, Record<string, Float32Array>>();

  // Side-table: per-vertex vector channels, mirroring meshScalarChannels.
  // Populated in createMeshFromData / updateMeshGeometry; deleted in removeMesh.
  const meshVectorChannels = new Map<string, Record<string, Float32Array>>();

  // Side-tables for deformation: cached original vertex positions and displaced positions.
  // Populated in createMeshFromData / updateMeshGeometry; deleted in removeMesh.
  // These are COPIES so in-place writes to the position buffer never corrupt the originals.
  const meshOriginalVertices = new Map<string, Float32Array>();
  const meshDisplacedPositions = new Map<string, Float32Array>();

  const meshMap = new Map<string, Mesh>();
  const visibilityMap = new Map<string, VisibilityState>();
  const ghostMeshMap = new Map<string, Mesh>();

  // Single shared ghost material — one material instance per manager, not per ghost clone.
  const ghostMaterial: MeshBasicMaterial = createGhostMaterial();

  // Shared undeformed-overlay material: transparent, low opacity, rendered behind deformed mesh.
  const undeformedMaterial = new MeshBasicMaterial({
    transparent: true,
    opacity: 0.25,
    depthWrite: false,
    side: DoubleSide,
  });

  // Ghost Group: all ghost clones live here so they're separate from opaque meshes.
  const ghostGroup = new Group();
  ghostGroup.name = 'ghostGroup';
  scene.add(ghostGroup);

  // Undeformed overlay Group: translucent original-position clones live here.
  const undeformedGroup = new Group();
  undeformedGroup.name = 'undeformedGroup';
  scene.add(undeformedGroup);

  // Map of active undeformed overlay meshes (one per entity with displaced_positions).
  const undeformedMeshMap = new Map<string, Mesh>();

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
    // The position buffer and the original-vertices side-table each need an
    // independent Float32Array: applyWarpToMesh mutates posAttr.array in-place,
    // so sharing a single copy would silently corrupt the side-table's canonical
    // pre-warp state.  One slice here; one at meshOriginalVertices.set below.
    const vertsForBuffer = data.vertices.slice();
    geometry.setAttribute('position', new BufferAttribute(vertsForBuffer, 3));
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
    // Store vector channels for sampleProbe interpolation.
    if (data.vector_channels) {
      meshVectorChannels.set(entityPath, data.vector_channels);
    }

    // Cache original vertices and displaced positions for deformation recompute.
    // We store copies so in-place blending into the position buffer never corrupts
    // the originals (needed for restore and re-apply at different warp factors).
    meshOriginalVertices.set(entityPath, data.vertices.slice());
    if (data.displaced_positions) {
      meshDisplacedPositions.set(entityPath, data.displaced_positions.slice());
    }

    const mesh = new Mesh(geometry, material);
    mesh.name = entityPath;

    // If deformation is already active, apply the warp to this freshly-created mesh
    // so a mid-stream backend sync doesn't snap the view back to undeformed.
    // Also add the undeformed overlay so a mesh that joins after the user toggles
    // 'Show deformed' is visually symmetric with meshes present at toggle time.
    if (currentDeformation !== null && data.displaced_positions) {
      applyWarpToMesh(mesh, entityPath, currentDeformation.warpFactor);
      // Warp runs for all meshes so the position buffer is correct if visibility
      // changes later. Overlay only for 'show' entities — hidden/ghost get none.
      if (isShown(entityPath)) {
        addUndeformedOverlay(entityPath, mesh);
      }
    }

    return mesh;
  }

  function updateMeshGeometry(mesh: Mesh, data: MeshData): void {
    const geometry = mesh.geometry as BufferGeometry;

    // Copy vertices on ingest — applyWarpToMesh mutates posAttr.array in-place;
    // aliasing data.vertices would clobber the caller's buffer.
    // meshOriginalVertices gets its own independent slice further below (line ~313)
    // so restore / re-apply at different warp factors works correctly.
    const vertsForBuffer = data.vertices.slice();

    // Reuse existing BufferAttribute objects when array length matches to avoid
    // orphaning GPU-side WebGLBuffers. When length differs, create new attribute
    // because WebGL buffers have fixed size and cannot be resized.
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (posAttr && posAttr.array.length === data.vertices.length) {
      posAttr.array = vertsForBuffer;
      (posAttr as { count: number }).count = data.vertices.length / 3;
      posAttr.needsUpdate = true;
    } else {
      geometry.setAttribute('position', new BufferAttribute(vertsForBuffer, 3));
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

    // Update the side-table scalar channels so setColorize picks up fresh data.
    // Use mesh.name as the entity path (set to entityPath in createMeshFromData).
    meshScalarChannels.set(mesh.name, data.scalar_channels ?? {});
    // Update vector channels (delete when absent so staleness detection is correct).
    if (data.vector_channels) {
      meshVectorChannels.set(mesh.name, data.vector_channels);
    } else {
      meshVectorChannels.delete(mesh.name);
    }

    // Refresh deformation side-tables from the incoming data.
    // Always update original vertices (vertices may change on a topology edit).
    meshOriginalVertices.set(mesh.name, data.vertices.slice());
    if (data.displaced_positions) {
      meshDisplacedPositions.set(mesh.name, data.displaced_positions.slice());
    } else {
      meshDisplacedPositions.delete(mesh.name);
    }

    // If colorize is active and this mesh already has a colour attribute, re-bake
    // the colours in place with the new scalars.  Mirrors the setColorize mutation
    // path; ensures that sync()→sync() updates (e.g. from G2 FEA sourcing) are
    // reflected immediately without a full geometry resync or material swap.
    if (colorize) {
      const scalars = (data.scalar_channels ?? {})[colorize.channel];
      const colorAttr = geometry.getAttribute('color') as BufferAttribute | null;
      if (scalars && scalars.length > 0 && colorAttr) {
        const newColors = colorize.bake(scalars);
        colorAttr.array = newColors;
        (colorAttr as { count: number }).count = newColors.length / 3;
        colorAttr.needsUpdate = true;
      }
    }

    // If deformation is active, re-apply warp and rebuild the overlay as needed.
    // The overlay's position BufferAttribute wraps the *previous* Float32Array from
    // meshOriginalVertices; after a topology-changing sync those entries are refreshed
    // above, but the overlay still points at the old array. Rebuilding it ensures the
    // ghost shape always matches the current un-displaced geometry.
    if (currentDeformation !== null) {
      if (data.displaced_positions) {
        applyWarpToMesh(mesh, mesh.name, currentDeformation.warpFactor);
        // Rebuild overlay: remove the stale one (if any) and create a fresh one
        // pointing at the just-updated meshOriginalVertices entry.
        // Remove stale overlay unconditionally (idempotent), then re-add only
        // for 'show' entities — hidden/ghost must not get an overlay.
        removeUndeformedOverlay(mesh.name);
        if (isShown(mesh.name)) {
          addUndeformedOverlay(mesh.name, mesh);
        }
      } else {
        // displaced_positions disappeared (e.g. backend turned off FEA solve):
        // remove any lingering overlay so no ghost shape persists for this entity.
        removeUndeformedOverlay(mesh.name);
      }
    }

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

  /**
   * Returns true iff `entityPath` is in the 'show' visibility state (the default when
   * absent from visibilityMap). Only 'show' entities receive an undeformed overlay;
   * 'hidden' and 'ghost' do not. Centralises the gate predicate used in
   * createMeshFromData, updateMeshGeometry, and setDeformation.
   */
  function isShown(entityPath: string): boolean {
    return (visibilityMap.get(entityPath) ?? 'show') === 'show';
  }

  /**
   * Write the linear-blend deformation into the position attribute of `mesh` for a
   * given `warpFactor`.  Reads from the cached original vertices and displaced_positions
   * side-tables; modifies `position.array` in-place and sets `needsUpdate = true`.
   *
   * Formula: pos[i] = orig[i] + W * (disp[i] - orig[i])
   * W=1 → exact displaced; W=0 → original; W>1 → amplified.
   */
  function applyWarpToMesh(mesh: Mesh, entityPath: string, warpFactor: number): void {
    const orig = meshOriginalVertices.get(entityPath);
    const disp = meshDisplacedPositions.get(entityPath);
    if (!orig || !disp) return;

    const geometry = mesh.geometry as BufferGeometry;
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (!posAttr) return;

    const arr = posAttr.array as Float32Array;
    for (let i = 0; i < arr.length; i++) {
      arr[i] = orig[i] + warpFactor * (disp[i] - orig[i]);
    }
    posAttr.needsUpdate = true;
  }

  /**
   * Restore the position attribute of `mesh` to the cached original vertices.
   */
  function restoreOriginalToMesh(mesh: Mesh, entityPath: string): void {
    const orig = meshOriginalVertices.get(entityPath);
    if (!orig) return;

    const geometry = mesh.geometry as BufferGeometry;
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (!posAttr) return;

    const arr = posAttr.array as Float32Array;
    for (let i = 0; i < arr.length; i++) {
      arr[i] = orig[i];
    }
    posAttr.needsUpdate = true;
  }

  /** Add a translucent undeformed overlay clone for the given entity. */
  function addUndeformedOverlay(entityPath: string, sourceMesh: Mesh): void {
    const orig = meshOriginalVertices.get(entityPath);
    if (!orig) return;

    // Fresh BufferGeometry that is fully self-owned:
    // - position points at the cached original vertices (warp writes go into the
    //   deformed mesh's position array and therefore never touch the overlay).
    // - index and normal are CLONED from the deformed mesh's geometry so that
    //   overlay.geometry.dispose() only frees the overlay's own GPU buffers and
    //   never invalidates the deformed mesh's VBOs. Without cloning, Three.js's
    //   WebGLRenderer.onGeometryDispose would walk every attribute on the disposed
    //   geometry and free its WebGLBuffer — including the index/normal buffers still
    //   referenced by the deformed mesh, causing silent per-frame VBO re-uploads.
    const overlayGeom = new BufferGeometry();
    overlayGeom.setAttribute('position', new BufferAttribute(orig, 3));
    const sourceGeom = sourceMesh.geometry as BufferGeometry;
    if (sourceGeom.index) {
      overlayGeom.setIndex(sourceGeom.index.clone());
    }
    const normalAttr = sourceGeom.getAttribute('normal');
    if (normalAttr) {
      overlayGeom.setAttribute('normal', (normalAttr as BufferAttribute).clone());
    }

    const overlay = new Mesh(overlayGeom, undeformedMaterial);
    overlay.name = `undeformed:${entityPath}`;
    overlay.renderOrder = -1; // draw behind the deformed mesh (renderOrder=0 default)

    undeformedMeshMap.set(entityPath, overlay);
    undeformedGroup.add(overlay);
  }

  /** Remove and dispose the undeformed overlay for the given entity (if any). */
  function removeUndeformedOverlay(entityPath: string): void {
    const overlay = undeformedMeshMap.get(entityPath);
    if (!overlay) return;
    undeformedGroup.remove(overlay);
    // Dispose the overlay's own BufferGeometry and its cloned index/normal attributes.
    // Because the overlay owns clones (not aliases) of the deformed mesh's attrs,
    // this frees only the overlay's GPU buffers and leaves the deformed mesh intact.
    overlay.geometry.dispose();
    undeformedMeshMap.delete(entityPath);
  }

  /**
   * Apply or remove a deformed-shape view across all managed meshes.
   * See MeshManagerContext.setDeformation for the full contract.
   */
  function setDeformation(opts: { warpFactor: number } | null): void {
    if (opts === null) {
      // Disable: restore all position buffers and tear down overlays.
      if (currentDeformation === null) return; // already inactive — no-op
      for (const [entityPath, mesh] of meshMap) {
        restoreOriginalToMesh(mesh, entityPath);
        removeUndeformedOverlay(entityPath);
      }
      currentDeformation = null;
      return;
    }

    // Early return when the same warpFactor is already active — avoids unnecessary
    // GPU-buffer writes and overlay teardown/re-add on redundant calls.
    // Note: the Viewport bridge effect is gated by track-then-act reactive tracking
    // (warpFactor is only read inside the showDeformed branch), so duplicate calls
    // with the same value are rare in practice, but the public API permits them.
    if (currentDeformation && currentDeformation.warpFactor === opts.warpFactor) return;

    // Enable (or re-apply with changed warpFactor): clear any existing overlays first
    // so a transition from one warpFactor to another doesn't leave stale overlays.
    for (const entityPath of [...undeformedMeshMap.keys()]) {
      removeUndeformedOverlay(entityPath);
    }

    currentDeformation = opts;
    const { warpFactor } = opts;

    for (const [entityPath, mesh] of meshMap) {
      const disp = meshDisplacedPositions.get(entityPath);
      if (!disp) continue; // no displaced_positions — skip (geometry untouched)

      applyWarpToMesh(mesh, entityPath, warpFactor);
      // Only 'show' state gets an overlay; hidden/ghost intentionally skipped.
      // See design decision: ghost is already a translucent deformed rendering;
      // hidden means the user wants no visual for this entity at all.
      if (isShown(entityPath)) {
        addUndeformedOverlay(entityPath, mesh);
      }
    }
  }

  function getDeformedOverlays(): Map<string, Mesh> {
    return new Map(undeformedMeshMap);
  }

  function removeMesh(entityPath: string): void {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return;

    const state = visibilityMap.get(entityPath) ?? 'show';

    // Remove from scene only if mesh is currently shown there
    if (state === 'show') {
      scene.remove(mesh);
    }

    // Remove the undeformed overlay first. Overlay disposal is no longer order-sensitive
    // (the overlay owns cloned index/normal attributes), but doing it first keeps teardown
    // deterministic and contrasts with the ghost-clone ordering requirement below.
    removeUndeformedOverlay(entityPath);

    // removeGhostClone MUST precede geometry disposal: the ghost clone shares
    // the original mesh's BufferGeometry reference. Disposing the geometry first
    // would leave the ghost clone referencing invalid GPU buffers.
    removeGhostClone(entityPath);

    (mesh.geometry as any).disposeBoundsTree();
    (mesh.geometry as BufferGeometry).dispose();
    (mesh.material as { dispose: () => void }).dispose();
    meshMap.delete(entityPath);
    meshScalarChannels.delete(entityPath);
    meshVectorChannels.delete(entityPath);
    meshOriginalVertices.delete(entityPath);
    meshDisplacedPositions.delete(entityPath);
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
        // Ghost is a translucent deformed rendering — a separate overlay is redundant.
        if (currentDeformation !== null) removeUndeformedOverlay(entityPath);
      } else if (state === 'hidden') {
        scene.remove(mesh);
        // Hidden means no visual at all — tear down the overlay if deformation is on.
        if (currentDeformation !== null) removeUndeformedOverlay(entityPath);
      }
    } else if (prevState === 'ghost') {
      if (state === 'show') {
        removeGhostClone(entityPath);
        scene.add(mesh);
        // Restore overlay now that entity is visible again.
        if (currentDeformation !== null && meshDisplacedPositions.has(entityPath)) {
          addUndeformedOverlay(entityPath, mesh);
        }
      } else if (state === 'hidden') {
        removeGhostClone(entityPath);
      }
    } else if (prevState === 'hidden') {
      if (state === 'show') {
        scene.add(mesh);
        // Overlay was absent while hidden — add it now that the entity is visible.
        if (currentDeformation !== null && meshDisplacedPositions.has(entityPath)) {
          addUndeformedOverlay(entityPath, mesh);
        }
      } else if (state === 'ghost') {
        addGhostClone(entityPath, mesh);
      }
    }
  }

  /**
   * Update the active colorize config and re-bake colour BufferAttributes in place
   * for every mesh that already has a `color` attribute (i.e. was created while a
   * colorize channel was active).
   *
   * **Asymmetric behaviour:** the material type (`MeshPhongMaterial` vs.
   * `MeshStandardMaterial`) is decided at mesh-creation time and is NOT changed
   * mid-stream by this call.  Key consequences:
   * - Meshes created while colorize was active keep their `MeshPhongMaterial`
   *   even after `setColorize(null)`; meshes synced after that call use
   *   `MeshStandardMaterial`.
   * - Meshes created while colorize was null will never get a colour attribute
   *   via `setColorize(opts)` alone; only meshes synced after the call will
   *   use `MeshPhongMaterial`.
   * Callers that need to flip all mesh materials uniformly must call `sync({})`
   * to drop all meshes, then re-sync the full dataset.
   *
   * When `opts` is null the colorize state is cleared; existing colour buffers
   * on already-phong meshes are left unchanged (material teardown is out of
   * scope for this task).
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

  /**
   * Rebuild mesh materials based on the current colorize state.
   * See `MeshManagerContext.rebuildMaterials` JSDoc for the full contract.
   */
  function rebuildMaterials(): void {
    for (const [entityPath, mesh] of meshMap) {
      const geometry = mesh.geometry as BufferGeometry;
      if (colorize === null) {
        // Null path: dispose old material, remove colour attr, install standard material.
        const oldMaterial = mesh.material as { dispose: () => void };
        geometry.deleteAttribute('color');
        mesh.material = new MeshStandardMaterial({
          color: colorForEntity(entityPath),
          side: DoubleSide,
        });
        oldMaterial.dispose();
      } else {
        // Set path: bake colour attribute and install phong material when channel present,
        // otherwise fall back to the standard material path (same as null branch above).
        const channels = meshScalarChannels.get(entityPath);
        const scalars = channels?.[colorize.channel];
        const oldMaterial = mesh.material as { dispose: () => void };
        if (scalars && scalars.length > 0) {
          const colors = colorize.bake(scalars);
          geometry.setAttribute('color', new BufferAttribute(colors, 3));
          mesh.material = new MeshPhongMaterial({
            vertexColors: true,
            flatShading: false,
            side: DoubleSide,
          });
        } else {
          geometry.deleteAttribute('color');
          mesh.material = new MeshStandardMaterial({
            color: colorForEntity(entityPath),
            side: DoubleSide,
          });
        }
        oldMaterial.dispose();
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
    // undeformedGroup and its shared material also need explicit scene removal.
    // Individual overlay meshes were already torn down by removeMesh above;
    // any remaining entries (edge case: overlays added without a matching meshMap entry)
    // are cleaned up here as a safety net.
    for (const entityPath of [...undeformedMeshMap.keys()]) {
      removeUndeformedOverlay(entityPath);
    }
    scene.remove(undeformedGroup);
    undeformedMaterial.dispose();
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

  /**
   * Compute barycentric coordinates of `point` within face `faceId` of entity
   * `entityPath`, reading the LIVE geometry position buffer.
   *
   * Uses the standard Cramer's-rule / projected-area formula (stable, UV-free).
   * Returns null when the entity is absent, the index buffer is missing, faceId
   * is out of range, or the triangle is degenerate (denom ≈ 0).
   */
  function computeBarycentric(
    entityPath: string,
    faceId: number,
    point: { x: number; y: number; z: number },
  ): BarycentricUV | null {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return null;

    const geometry = mesh.geometry as BufferGeometry;
    const indexAttr = geometry.index;
    if (!indexAttr) return null;

    const indexArr = indexAttr.array as Uint32Array | Uint16Array | number[];
    const base = 3 * faceId;
    if (base + 2 >= indexArr.length) return null;

    const ia = indexArr[base];
    const ib = indexArr[base + 1];
    const ic = indexArr[base + 2];

    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (!posAttr) return null;
    const pos = posAttr.array as Float32Array;

    // Vertex positions
    const ax = pos[ia * 3], ay = pos[ia * 3 + 1], az = pos[ia * 3 + 2];
    const bx = pos[ib * 3], by = pos[ib * 3 + 1], bz = pos[ib * 3 + 2];
    const cx = pos[ic * 3], cy = pos[ic * 3 + 1], cz = pos[ic * 3 + 2];

    // Edge vectors
    const v0x = bx - ax, v0y = by - ay, v0z = bz - az;
    const v1x = cx - ax, v1y = cy - ay, v1z = cz - az;
    const v2x = point.x - ax, v2y = point.y - ay, v2z = point.z - az;

    // Dot products
    const d00 = v0x * v0x + v0y * v0y + v0z * v0z;
    const d01 = v0x * v1x + v0y * v1y + v0z * v1z;
    const d11 = v1x * v1x + v1y * v1y + v1z * v1z;
    const d20 = v2x * v0x + v2y * v0y + v2z * v0z;
    const d21 = v2x * v1x + v2y * v1y + v2z * v1z;

    const denom = d00 * d11 - d01 * d01;
    if (Math.abs(denom) < 1e-12) return null; // degenerate triangle

    const v = (d11 * d20 - d01 * d21) / denom;
    const w = (d00 * d21 - d01 * d20) / denom;
    const u = 1 - v - w;

    return [u, v, w];
  }

  /**
   * Sample FEA quantities at a probe point given its barycentric coordinates.
   *
   * Returns null when the entity is absent or faceId is out of range (the
   * staleness signal passed back to resampleAll()).
   */
  function sampleProbe(
    entityPath: string,
    faceId: number,
    bary: BarycentricUV,
  ): ProbeSample | null {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return null;

    const geometry = mesh.geometry as BufferGeometry;
    const indexAttr = geometry.index;
    if (!indexAttr) return null;

    const indexArr = indexAttr.array as Uint32Array | Uint16Array | number[];
    const base = 3 * faceId;
    if (base + 2 >= indexArr.length) return null;

    const ia = indexArr[base];
    const ib = indexArr[base + 1];
    const ic = indexArr[base + 2];

    const [u, v, w] = bary;

    // -----------------------------------------------------------------------
    // Displacement: barycentric-interp of (displaced - original) per vertex
    // -----------------------------------------------------------------------
    let displacement: [number, number, number] | null = null;
    const orig = meshOriginalVertices.get(entityPath);
    const disp = meshDisplacedPositions.get(entityPath);
    if (orig && disp) {
      const dx = u * (disp[ia * 3]     - orig[ia * 3])     +
                 v * (disp[ib * 3]     - orig[ib * 3])     +
                 w * (disp[ic * 3]     - orig[ic * 3]);
      const dy = u * (disp[ia * 3 + 1] - orig[ia * 3 + 1]) +
                 v * (disp[ib * 3 + 1] - orig[ib * 3 + 1]) +
                 w * (disp[ic * 3 + 1] - orig[ic * 3 + 1]);
      const dz = u * (disp[ia * 3 + 2] - orig[ia * 3 + 2]) +
                 v * (disp[ib * 3 + 2] - orig[ib * 3 + 2]) +
                 w * (disp[ic * 3 + 2] - orig[ic * 3 + 2]);
      displacement = [dx, dy, dz];
    }

    // -----------------------------------------------------------------------
    // Scalar channels (vonMises surfaced explicitly; all others in scalars)
    // -----------------------------------------------------------------------
    const scalars: Record<string, number> = {};
    let vonMises: number | null = null;
    const scalarMap = meshScalarChannels.get(entityPath);
    if (scalarMap) {
      for (const [name, arr] of Object.entries(scalarMap)) {
        const val = u * arr[ia] + v * arr[ib] + w * arr[ic];
        scalars[name] = val;
        if (name === 'vonMises') vonMises = val;
      }
    }

    // -----------------------------------------------------------------------
    // Vector channels (component-wise interpolation)
    // -----------------------------------------------------------------------
    const vectors: Record<string, [number, number, number]> = {};
    const vecMap = meshVectorChannels.get(entityPath);
    if (vecMap) {
      for (const [name, arr] of Object.entries(vecMap)) {
        const vx = u * arr[ia * 3]     + v * arr[ib * 3]     + w * arr[ic * 3];
        const vy = u * arr[ia * 3 + 1] + v * arr[ib * 3 + 1] + w * arr[ic * 3 + 1];
        const vz = u * arr[ia * 3 + 2] + v * arr[ib * 3 + 2] + w * arr[ic * 3 + 2];
        vectors[name] = [vx, vy, vz];
      }
    }

    return { displacement, vonMises, scalars, vectors };
  }

  return {
    sync,
    dispose,
    getSceneMeshes,
    setVisibility,
    getGhostMeshes,
    setColorize,
    rebuildMaterials,
    setDeformation,
    getDeformedOverlays,
    computeBarycentric,
    sampleProbe,
  };
}
