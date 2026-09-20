/**
 * orbitDistance.ts — the orbit-distance policy for OrbitControls.
 *
 * Deliberately dependency-free (no `three` import): `fitCamera.ts` consumes this
 * policy, and `selection.test.ts` exercises `fitCamera.ts` behind a PARTIAL
 * `vi.mock('three')`.  Pulling `controls.ts` in instead — the other obvious home —
 * would drag the real OrbitControls module through that partial mock at import time
 * and break an unrelated suite.  A pure module also keeps the policy independently
 * testable and gives both distance limits one home (SPOT).
 *
 * The scale-free invariant
 * ────────────────────────
 * `fitCameraToBox` frames the sphere circumscribing the model with padding 1.1, and
 * the scene camera's vertical FOV is 60°, so a fitted camera always sits at
 * `1.1 · radius / sin(30°) = 2.2 · radius` from its target.  Expressing the floor as
 * a FRACTION of that same radius therefore fixes it at `1/110` of the fitted
 * distance for every model at every scale: anything that can be framed can be
 * dollied in ~110× from its fitted pose.
 *
 * What this replaces, and why a fraction rather than a smaller constant: the floor
 * used to be an absolute 0.5 m in a workspace whose parts span four orders of
 * magnitude.  A 75 mm probe fits at ~86 mm, so the floor silently relocated the
 * camera ~6× too far out and made `zoom_camera` a no-op (#6496).  Any absolute value
 * merely moves that cliff to some other part size; only a fraction cannot recur.
 */

/**
 * Fraction of the framed bounding-sphere radius the camera may approach its target.
 * 0.02 · radius is 1/110 of the fitted distance — see the scale-free invariant above.
 */
export const ORBIT_MIN_DISTANCE_FRACTION = 0.02;

/**
 * Strictly-positive absolute floor, used when no radius is available (startup, before
 * any geometry exists) or when one is degenerate.  A zero or non-finite floor would
 * make OrbitControls' `_clampDistance` produce NaN camera positions.
 */
export const ORBIT_MIN_DISTANCE_FLOOR = 1e-6;

/** Unchanged from the previous hardcoded value; re-homed here so both limits sit together. */
export const ORBIT_MAX_DISTANCE = 500;

/**
 * The minimum orbit distance for a model whose bounding-sphere radius is `radius`.
 *
 * A non-finite or non-positive radius yields `ORBIT_MIN_DISTANCE_FLOOR` rather than
 * 0 or NaN, so a degenerate model can never poison the clamp.
 */
export function orbitMinDistanceFor(radius: number): number {
  if (!Number.isFinite(radius) || radius <= 0) return ORBIT_MIN_DISTANCE_FLOOR;
  return Math.max(radius * ORBIT_MIN_DISTANCE_FRACTION, ORBIT_MIN_DISTANCE_FLOOR);
}
