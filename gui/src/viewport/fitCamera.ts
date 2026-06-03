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
 * 3. Explicit padding factor (DEFAULT_FIT_PADDING ≈ 1.1)
 *    A small multiplicative padding ensures the assembly never touches the
 *    frame edges, giving a natural margin for the elongated rod/tendon
 *    assemblies typical in Reify projects.  The exact constant can be tuned
 *    without breaking the test suite, which asserts qualitative containment
 *    (strict inside-frame margin + not-a-speck) rather than the exact value.
 *
 * 4. Preserved view direction
 *    The camera is repositioned along its existing view direction vector, so
 *    the orientation the user last set (pan/orbit) is retained.  Only the
 *    distance changes.
 */

import { Vector3 } from 'three';
import type { Box3, PerspectiveCamera } from 'three';

const DEFAULT_FIT_PADDING = 1.1;

export interface FitCameraOptions {
  /** OrbitControls (or any object with a copyable Vector3 `target`). */
  controls?: { target: { copy: (v: Vector3) => void } };
  /** Multiplicative padding around the bounding sphere (default 1.1). */
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

  const padding = options?.padding ?? DEFAULT_FIT_PADDING;

  // Vertical FOV in radians.
  const vFov = (camera.fov * Math.PI) / 180;

  // Horizontal FOV derived from the vertical FOV and the aspect ratio.
  // When aspect < 1 (tall/narrow pane), hFov < vFov — the horizontal
  // constraint is tighter and fitW dominates.
  const aspect = camera.aspect ?? 1;
  const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect);

  // Candidate distances: how far back must the camera be for the sphere to
  // fit inside the vertical / horizontal half-angles respectively?
  const fitH = radius / Math.sin(vFov / 2);
  const fitW = radius / Math.sin(hFov / 2);

  const distance = padding * Math.max(fitH, fitW);

  // Reposition along the current view direction, preserving orientation.
  const viewDir = new Vector3();
  camera.getWorldDirection(viewDir);
  camera.position.copy(center).sub(viewDir.multiplyScalar(distance));
  camera.lookAt(center);
  camera.updateProjectionMatrix();

  // Sync OrbitControls target so it orbits around the framed assembly.
  options?.controls?.target.copy(center);
}
