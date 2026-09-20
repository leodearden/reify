/**
 * orbitDistance.ts — the camera-distance policy: how far the camera sits from its
 * target when a subject is framed, and how close it may then be dollied in.
 *
 * Deliberately dependency-free (no `three` import).  Three consequences, all load-bearing:
 *   • `fitCamera.ts` consumes this policy, and `selection.test.ts` exercises `fitCamera.ts`
 *     behind a PARTIAL `vi.mock('three')`.  Putting the policy in `controls.ts` — the other
 *     obvious home — would drag the real OrbitControls module through that partial mock at
 *     import time and break an unrelated suite.
 *   • The e2e harness (`gui/test/visual/assertions.ts`, run under bare `tsx`, not vitest)
 *     can import the real formula to derive its expectations instead of restating them.
 *   • Both distance limits, and the framing distance they are stated against, have ONE
 *     home, so no caller has to re-derive the relation between them (SPOT).
 *
 * The scale-free invariant
 * ────────────────────────
 * `fittedDistanceFor` is the single implementation of "how far back must the camera be to
 * frame a sphere of this radius" — `fitCameraToBox` calls it, and the floor below is stated
 * as a FRACTION of its result rather than of the radius.  So the guarantee "anything that
 * can be framed can then be dollied in ~110× from its fitted pose" holds at every model
 * scale, at every field of view and for every pane shape, BY CONSTRUCTION: retune the
 * padding or the FOV and the floor follows, because neither number appears here twice.
 *
 * What this replaces, and why a fraction rather than a smaller constant: the floor used to
 * be an absolute 0.5 m in a workspace whose parts span four orders of magnitude.  A 75 mm
 * probe fits at ~86 mm, so the floor silently relocated the camera ~6× too far out and made
 * `zoom_camera` a no-op (#6496).  Any absolute value merely moves that cliff to some other
 * part size; only a fraction cannot recur.
 */

/**
 * Multiplicative padding around the framed bounding sphere, so the subject never touches
 * the frame edges.  Tunable: the suite asserts qualitative containment (strict inside-frame
 * margin + not-a-speck) and derives every fitted distance from `fittedDistanceFor`, so no
 * test pins this value.
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
 * dolly-in headroom a framed subject is guaranteed.  See the scale-free invariant above.
 */
export const ORBIT_MIN_DISTANCE_FRACTION_OF_FIT = 1 / 110;

/**
 * Strictly-positive absolute floor, used only when the framed distance is degenerate
 * (zero, negative or non-finite).  A zero or non-finite floor would make OrbitControls'
 * `_clampDistance` produce NaN camera positions, which is unrecoverable without a reload.
 *
 * This is the NaN guard and nothing else — it is deliberately NOT what `createControls`
 * seeds.  Seeding it would leave an empty scene's wheel able to dolly to a 1e-6 orbit
 * radius, and since dolly is multiplicative (and pan magnitude scales with distance),
 * climbing back out takes ~160 wheel ticks: a soft-lock escapable only via `fit_to_view`.
 */
export const ORBIT_MIN_DISTANCE_FLOOR = 1e-6;

/**
 * The far limit, deliberately left an ABSOLUTE distance rather than model-derived.
 *
 * Unlike the floor it is not implicated by #6496 and has no scale cliff in reach: it binds
 * only on a model whose bounding sphere exceeds ~227 m in radius (≈455 m across), since
 * such a model's own fitted distance would exceed it and `_clampDistance` would pull the
 * framing inward.  Reify parts are four orders of magnitude smaller than that.  Should a
 * model that large ever appear, the fix is the same one the floor received — derive it from
 * `fittedDistanceFor` — not a bigger constant.
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
