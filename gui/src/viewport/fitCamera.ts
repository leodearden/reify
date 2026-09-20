/**
 * fitCamera.ts — pure camera-framing helper.
 *
 * Exposes `fitCameraToBox(camera, box, opts?)` which repositions a
 * PerspectiveCamera so that the given axis-aligned bounding box is fully
 * contained in the view frustum with a comfortable padding margin.
 *
 * Design decisions
 * ────────────────
 * 1. Bounding-sphere framing (view-direction-independent)
 *    Distance is computed from the sphere that circumscribes the box
 *    (radius = ½ · box diagonal derived from Box3.getSize).  Framing the
 *    sphere guarantees no clipping at any camera orientation, including the
 *    default iso orbit used in the Reify design pane.  The radius is
 *    intentionally derived from getSize (NOT getBoundingSphere) so the
 *    helper is compatible with the hand-rolled three mock in
 *    selection.test.ts, which lacks getBoundingSphere and Vector3.length().
 *
 * 2. Aspect-aware distance (fix for esc-4280 over-zoom)
 *    The old selection.ts formula fit maxDim to the vertical FOV only and
 *    ignored camera.aspect.  On the tall/narrow design pane (aspect < 1)
 *    the horizontal FOV is narrower than the vertical FOV, so the horizontal
 *    extent is the binding constraint.  By computing a candidate distance
 *    from BOTH the vertical FOV and the horizontal FOV and taking the
 *    maximum, the camera is placed far enough back to frame the assembly
 *    correctly for any aspect ratio.
 *
 * 3. The framing distance itself lives in orbitDistance.ts
 *    `fittedDistanceFor` owns the padding constant and the two-half-angle
 *    trigonometry (decision 2 above); this module keeps only the three-specific
 *    work — box → bounding-sphere radius, and repositioning.
 *    That split is what lets the orbit floor be stated as a fraction of the
 *    fitted distance without either module importing the other, and stops the
 *    padding/FOV relation from being re-derived in the tests (SPOT).
 *
 * 4. Preserved view direction
 *    The camera is repositioned along its existing view direction vector, so
 *    the orientation the user last set (pan/orbit) is retained.  Only the
 *    distance changes.
 *
 * 5. Framing also sets the orbit distance floor (task 6965)
 *    Framing a model and deciding how close the user may then get to it are the
 *    same question asked twice, so both are answered from the ONE `distance`
 *    computed here — not from two independent derivations that could drift
 *    apart.  Only the near limit is model-derived; `ORBIT_MAX_DISTANCE` is
 *    deliberately absolute (see orbitDistance.ts for the size at which it would
 *    bind).  The write sits AFTER the degenerate-box early return, so the
 *    documented "a degenerate box mutates no controls state" contract covers
 *    minDistance exactly as it already covers target.
 */

import { Vector3 } from 'three';
import type { Box3, PerspectiveCamera } from 'three';
import { fittedDistanceFor, orbitFloorFor } from './orbitDistance';

export interface FitCameraOptions {
  /**
   * OrbitControls (or any object with a copyable Vector3 `target`).  `minDistance`
   * is written, not read, so a caller may omit it.
   */
  controls?: { target: { copy: (v: Vector3) => void }; minDistance?: number };
  /** Multiplicative padding around the bounding sphere (default DEFAULT_FIT_PADDING). */
  padding?: number;
}

/**
 * Reposition `camera` so that the bounding sphere of `box` fits inside the
 * view frustum with a padding margin.  The current view direction is
 * preserved; only the distance from the box center changes.
 *
 * When `options.controls` is provided, `controls.target` is updated to the
 * box center (required for OrbitControls to orbit around the framed object).
 *
 * No-ops if `box` is empty or degenerate (zero-volume).
 */
export function fitCameraToBox(
  camera: PerspectiveCamera,
  box: Box3,
  options?: FitCameraOptions,
): void {
  const center = new Vector3();
  const size = new Vector3();
  box.getCenter(center);
  box.getSize(size);

  // Bounding-sphere radius from half the box diagonal.
  // Using Math.sqrt of half-extents avoids getBoundingSphere (mock compat).
  const radius = 0.5 * Math.sqrt(
    size.x * size.x + size.y * size.y + size.z * size.z,
  );

  // Guard against empty / degenerate boxes.
  if (!(radius > 0)) return;

  const distance = fittedDistanceFor(radius, camera.fov, camera.aspect ?? 1, options?.padding);

  // Reposition along the current view direction, preserving orientation.
  // Refresh world matrix first so getWorldDirection reads current state — callers
  // may have mutated position/rotation without calling updateMatrixWorld().
  // Optional chaining keeps the helper compatible with hand-rolled test mocks
  // that don't stub every camera method.
  camera.updateMatrixWorld?.();
  const viewDir = new Vector3();
  camera.getWorldDirection(viewDir);
  camera.position.copy(center).sub(viewDir.multiplyScalar(distance));
  camera.lookAt(center);
  camera.updateProjectionMatrix();

  // Sync OrbitControls state to the framed assembly: the target it orbits around,
  // and the floor on how close the user may then get to it (design decision 5).
  if (options?.controls) {
    options.controls.target.copy(center);
    options.controls.minDistance = orbitFloorFor(distance);
  }
}
