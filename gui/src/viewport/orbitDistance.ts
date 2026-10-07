/**
 * orbitDistance.ts — the camera-distance policy: how far the camera sits from its
 * target when a subject is framed, and how close it may then be dollied in.
 * Contract and rationale: docs/debug-mcp-contract.md §6 point 3.
 *
 * Keep this module dependency-free (no `three`, no DOM).  `fitCamera.ts` imports it and
 * `selection.test.ts` exercises `fitCamera.ts` behind a PARTIAL `vi.mock('three')`, and
 * the e2e harness (`gui/test/visual/assertions.ts`) imports it under bare `tsx`.
 */

/**
 * Multiplicative padding around the framed bounding sphere, so the subject never touches
 * the frame edges.  Tunable: no test pins the value.
 */
export const DEFAULT_FIT_PADDING = 1.1;

/**
 * How far a camera must sit from the centre of a sphere of `radius` for that sphere to fit
 * inside the view frustum with `padding` margin.
 *
 * Both half-angles are considered and the larger distance wins: on a pane TALLER than wide
 * (`aspect < 1`) the horizontal FOV is the narrower one and binds instead (esc-4280).
 * `aspect` defaults to 1, the square-pane reference.
 */
export function fittedDistanceFor(
  radius: number,
  fovDeg: number,
  aspect: number = 1,
  padding: number = DEFAULT_FIT_PADDING,
): number {
  const vFov = (fovDeg * Math.PI) / 180;
  const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect);
  const fitH = radius / Math.sin(vFov / 2);
  const fitW = radius / Math.sin(hFov / 2);
  return padding * Math.max(fitH, fitW);
}

/**
 * Fraction of the FRAMED distance the camera may approach its target — equivalently, the
 * dolly-in headroom a framed subject is guaranteed, at any model scale.
 */
export const ORBIT_MIN_DISTANCE_FRACTION_OF_FIT = 1 / 110;

/**
 * Strictly-positive absolute floor, used only when the framed distance is degenerate
 * (zero, negative or non-finite).  A zero or non-finite floor would make OrbitControls'
 * `_clampDistance` produce NaN camera positions, which is unrecoverable without a reload.
 * A NaN guard only: never seed `minDistance` with it directly.
 */
export const ORBIT_MIN_DISTANCE_FLOOR = 1e-6;

/**
 * The far limit, deliberately an ABSOLUTE distance rather than model-derived; where it
 * binds is stated in docs/debug-mcp-contract.md §6 point 3.
 */
export const ORBIT_MAX_DISTANCE = 500;

/**
 * The closest a camera may orbit to its target, given the distance it sits at with the
 * subject framed (`fittedDistanceFor`, or the live orbit distance before anything has been
 * framed).
 *
 * A non-finite or non-positive input yields `ORBIT_MIN_DISTANCE_FLOOR` rather than 0 or
 * NaN, so a degenerate model can never poison the clamp.
 */
export function orbitFloorFor(framedDistance: number): number {
  if (!Number.isFinite(framedDistance) || framedDistance <= 0) return ORBIT_MIN_DISTANCE_FLOOR;
  return Math.max(framedDistance * ORBIT_MIN_DISTANCE_FRACTION_OF_FIT, ORBIT_MIN_DISTANCE_FLOOR);
}
