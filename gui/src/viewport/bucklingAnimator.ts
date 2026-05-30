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
  /** Write new positions into the GPU buffer in place. */
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
    for (let i = 0; i < positions.length; i++) {
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
