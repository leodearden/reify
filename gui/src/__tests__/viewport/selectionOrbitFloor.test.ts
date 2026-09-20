/**
 * Does the SHIPPED fitToView path actually establish the orbit floor? (#6496)
 *
 * Every other test of the floor supplies its own wiring: `fitCamera.test.ts` calls
 * `fitCameraToBox` directly, and `debugContract.test.ts` installs a viewport whose
 * `fitToView` is a hand-rolled stand-in for selection.ts's.  Neither observes the real
 * `createSelection().fitToView()`, so if selection.ts stopped forwarding `controls` — or
 * forwarded a wrapper without a writable `minDistance` — the whole fix would go inert in
 * production with the entire suite still green.  That seam is what this file pins, and
 * it is the only thing it pins; framing geometry belongs to fitCamera.test.ts.
 *
 * REAL three (no `vi.mock('three')`): the seam under test is whether a real Box3 built
 * from a real Mesh reaches `fitCameraToBox` with the caller's controls object attached,
 * and a mocked Box3/Vector3 would let a broken forward still look right.  `controls` is
 * a plain stub rather than real OrbitControls because OrbitControls' own `_clampDistance`
 * is not in scope here — only the write is.
 */

import { describe, it, expect } from 'vitest';
import {
  Scene,
  PerspectiveCamera,
  Mesh,
  BoxGeometry,
  MeshStandardMaterial,
  Vector3,
} from 'three';
import { createSelection } from '../../viewport/selection';
import { CAMERA_FOV_DEG } from '../../viewport/scene';
import { fittedDistanceFor, orbitFloorFor } from '../../viewport/orbitDistance';

/** The 75 mm probe from the #6496 report — the scale the old 0.5 m floor swallowed. */
const PROBE = { x: 0.075, y: 0.02, z: 0.01 };
const RADIUS = 0.5 * Math.hypot(PROBE.x, PROBE.y, PROBE.z);
const ASPECT = 800 / 600;

/**
 * The expectations below are compared as RATIOS, not with an absolute `toBeCloseTo`
 * tolerance.  BufferGeometry stores vertex positions as float32, so the Box3 that
 * `fitToView` builds from a real Mesh carries ~1e-8 relative rounding against the exact
 * `PROBE` dimensions — an absolute tolerance tight enough to be meaningful at the 1e-4
 * scale of the floor would fail on that rounding alone, while one loose enough to pass
 * would be vacuous at the 1e-2 scale of the fitted distance.
 */
const FLOAT32_RELATIVE_DIGITS = 6;

/**
 * A sentinel no policy value could coincide with, so "rewritten" and "happened to already
 * hold the right number" stay distinguishable.  0.5 is also the exact value #6496 was
 * about, which makes a regression to the old behaviour read as an unchanged sentinel.
 */
const SENTINEL_MIN_DISTANCE = 0.5;

function setup(withControls: boolean) {
  const scene = new Scene();
  const camera = new PerspectiveCamera(CAMERA_FOV_DEG, ASPECT, 0.1, 10000);
  camera.position.set(0, 0, 1);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld();

  const mesh = new Mesh(
    new BoxGeometry(PROBE.x, PROBE.y, PROBE.z),
    new MeshStandardMaterial(),
  );
  mesh.name = 'entity/probe';
  mesh.updateMatrixWorld();
  scene.add(mesh);

  const domElement = document.createElement('canvas');
  const controls = { target: new Vector3(), minDistance: SENTINEL_MIN_DISTANCE };

  const selection = createSelection({
    scene,
    camera,
    domElement,
    getMeshes: () => new Map([['entity/probe', mesh]]),
    onHover: () => {},
    onSelect: () => {},
    ...(withControls ? { controls } : {}),
  } as Parameters<typeof createSelection>[0]);

  return { selection, camera, controls };
}

describe('selection.fitToView forwards controls far enough to establish the orbit floor', () => {
  it('rewrites minDistance below the distance it just framed at', () => {
    const { selection, camera, controls } = setup(true);

    selection.fitToView();

    const framedDistance = camera.position.distanceTo(controls.target);
    expect(controls.minDistance).not.toBe(SENTINEL_MIN_DISTANCE);
    expect(controls.minDistance).toBeGreaterThan(0);
    expect(controls.minDistance).toBeLessThan(framedDistance);
  });

  it('the value written is the policy applied to this model, not some other scale', () => {
    const { selection, controls } = setup(true);

    selection.fitToView();

    const expected = orbitFloorFor(fittedDistanceFor(RADIUS, CAMERA_FOV_DEG, ASPECT));
    expect(controls.minDistance / expected).toBeCloseTo(1, FLOAT32_RELATIVE_DIGITS);
  });

  it('the floor it writes leaves the framed 75 mm probe reachable, unlike the old 0.5 m', () => {
    const { selection, camera, controls } = setup(true);

    selection.fitToView();

    // The regression in one line: the part frames INSIDE the distance the old absolute
    // floor would have pinned it at, so that floor made the part unreachable.
    expect(camera.position.distanceTo(controls.target)).toBeLessThan(SENTINEL_MIN_DISTANCE);
    expect(controls.minDistance).toBeLessThan(SENTINEL_MIN_DISTANCE);
  });

  it('still frames without controls — the forward is optional, not required', () => {
    const { selection, camera, controls } = setup(false);

    expect(() => selection.fitToView()).not.toThrow();
    // Non-vacuity check for the three cases above: the identical stub, merely not handed
    // to createSelection, comes back untouched.  So those assertions observe the forward
    // itself and not some ambient write.
    expect(controls.minDistance).toBe(SENTINEL_MIN_DISTANCE);
    // Framed to the probe: the camera moved off its (0,0,1) start towards the part.
    const expected = fittedDistanceFor(RADIUS, CAMERA_FOV_DEG, ASPECT);
    expect(camera.position.distanceTo(new Vector3(0, 0, 0)) / expected).toBeCloseTo(
      1,
      FLOAT32_RELATIVE_DIGITS,
    );
  });
});
