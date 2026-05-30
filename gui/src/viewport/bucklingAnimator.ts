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
