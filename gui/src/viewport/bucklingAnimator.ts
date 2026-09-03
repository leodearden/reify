/**
 * BucklingAnimator — connectivity-free point-cloud display for the buckling
 * mode-shape animator.  Task ι/3458.
 *
 * The buckling solver builds its own internal tet-grid whose nodes are a
 * different vertex set from the OCCT B-rep tessellation in the main viewport.
 * No FEA-node→OCCT-vertex mapping exists, so a simple Points representation
 * (no connectivity, no normals, no indexing) is the self-contained option.
 *
 * Pattern mirrors meshManager.applyWarpToMesh: get 'position' BufferAttribute,
 * write array in-place, set needsUpdate = true.
 */

import {
  BufferGeometry,
  Float32BufferAttribute,
  Points,
  PointsMaterial,
} from 'three';

// ---------------------------------------------------------------------------
// Bounds helper
// ---------------------------------------------------------------------------

/**
 * Compute the bounding-box center and half-space-diagonal radius for a flat
 * XYZ position array.  Pure function — no three.js dependency.
 *
 * Deliberately avoids three.Box3 / BufferGeometry.computeBoundingSphere so
 * the helper is unit-testable with plain numbers (no WebGL infrastructure)
 * and keeps the BucklingPanel.test.tsx mock surface minimal (only
 * Scene/Camera/Renderer need stubs, not Box3/Vector3).  For the three.Box3
 * pattern used elsewhere see: viewport/scene.ts, viewport/selection.ts,
 * gui/src/debug/bridge.ts.
 *
 * Returns { center:[0,0,0], radius:0 } for an empty / zero-length input.
 */
export function computePointCloudBounds(
  positions: number[],
): { center: [number, number, number]; radius: number } {
  if (positions.length === 0) return { center: [0, 0, 0], radius: 0 };

  let xMin = Infinity, xMax = -Infinity;
  let yMin = Infinity, yMax = -Infinity;
  let zMin = Infinity, zMax = -Infinity;

  for (let i = 0; i < positions.length; i += 3) {
    const x = positions[i]!;
    const y = positions[i + 1]!;
    const z = positions[i + 2]!;
    if (x < xMin) xMin = x; if (x > xMax) xMax = x;
    if (y < yMin) yMin = y; if (y > yMax) yMax = y;
    if (z < zMin) zMin = z; if (z > zMax) zMax = z;
  }

  const cx = (xMin + xMax) / 2;
  const cy = (yMin + yMax) / 2;
  const cz = (zMin + zMax) / 2;
  const dx = xMax - xMin, dy = yMax - yMin, dz = zMax - zMin;
  const radius = 0.5 * Math.sqrt(dx * dx + dy * dy + dz * dz);

  return { center: [cx, cy, cz], radius };
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

export interface BucklingAnimator {
  /** The displaced point-cloud Object3D; add to a scene to render. */
  object3d: Points;
  /** The undeformed reference overlay; toggle .visible to show/hide. */
  undeformedOverlay: Points;
  /**
   * Write new positions into the GPU buffer in place.
   *
   * The buffer is fixed-size from construction — it is sized once from the
   * `base` passed to `createBucklingAnimator` and is never reallocated, because
   * WebGL buffers cannot be resized (#6757). A `positions` whose length differs
   * from that buffer is therefore clamped to the buffer length and warned about
   * rather than throwing (an animation tick must not break the render loop): an
   * over-long array has its tail dropped, a short one leaves a stale tail.
   */
  update(positions: number[]): void;
  /** Show or hide the undeformed (reference) overlay. */
  setUndeformedVisible(visible: boolean): void;
  /** Dispose GPU resources (geometry + material for both objects). */
  dispose(): void;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create a BucklingAnimator seeded with the undeformed node positions.
 *
 * @param base Flat XYZ array of undeformed node positions (length = 3·n_nodes).
 */
export function createBucklingAnimator(base: number[]): BucklingAnimator {
  // ── Displaced point-cloud (primary, updated on every animation tick) ────
  const dispArray = new Float32Array(base);
  const dispGeom = new BufferGeometry();
  dispGeom.setAttribute('position', new Float32BufferAttribute(dispArray, 3));

  const dispMaterial = new PointsMaterial({ color: 0x4488ff, size: 4 });
  const displaced = new Points(dispGeom, dispMaterial);

  // ── Undeformed reference overlay (static, hidden by default) ────────────
  const baseArray = new Float32Array(base);
  const baseGeom = new BufferGeometry();
  baseGeom.setAttribute('position', new Float32BufferAttribute(baseArray, 3));

  const baseMaterial = new PointsMaterial({ color: 0xaaaaaa, size: 2 });
  const undeformed = new Points(baseGeom, baseMaterial);
  undeformed.visible = false;

  // ── Methods ──────────────────────────────────────────────────────────────

  function update(positions: number[]): void {
    const posAttr = dispGeom.getAttribute('position') as Float32BufferAttribute;
    const arr = posAttr.array as Float32Array;
    // The destination is fixed-size: it was allocated once from `base` above
    // and cannot be grown, because WebGL buffers have fixed size (#6757). So
    // bound the copy by the DESTINATION, not the source. Bounding by the source
    // was safe only by accident — TypedArray out-of-bounds writes happen to be
    // silent no-ops, a property of the array type rather than of this code.
    //
    // Warn rather than throw: update() runs on every animation tick, so a throw
    // would turn a cosmetically-wrong frame into a broken render loop. This
    // follows validateMeshData's convention for caller-supplied array-length
    // inconsistencies (meshManager.ts) — name the offending length and what it
    // was compared against, then degrade gracefully (#6813).
    if (positions.length !== arr.length) {
      console.warn(
        `bucklingAnimator.update(): positions.length (${positions.length}) != ` +
          `position buffer length (${arr.length}); ` +
          `writing ${Math.min(positions.length, arr.length)} values, buffer not resized`,
      );
    }
    const n = Math.min(positions.length, arr.length);
    for (let i = 0; i < n; i++) {
      arr[i] = positions[i]!;
    }
    posAttr.needsUpdate = true;
  }

  function setUndeformedVisible(visible: boolean): void {
    undeformed.visible = visible;
  }

  function dispose(): void {
    dispGeom.dispose();
    dispMaterial.dispose();
    baseGeom.dispose();
    baseMaterial.dispose();
  }

  return {
    object3d: displaced,
    undeformedOverlay: undeformed,
    update,
    setUndeformedVisible,
    dispose,
  };
}
