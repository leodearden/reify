/**
 * orbitUpAxis.test.ts — the OrbitControls orbit-frame / camera.up coherence contract (#6497).
 *
 * REAL three and a REAL OrbitControls, with NO vi.mock: the whole subject is the
 * library's constructor-time private quaternion.  A hand-rolled stub has no `_quat`, so
 * a test written against one would pass in both the broken and the fixed state.
 *
 * The defect: three 0.183.2 derives its orbit frame ONCE, in the constructor
 * (OrbitControls.js:406), from `object.up`.  `_quat` is assigned only there and read only
 * at :695 and :784; neither `update()` nor `reset()` re-derives it, and no public API
 * does.  So mutating `camera.up` afterwards — which is exactly what `set_camera { up }`
 * does — leaves `camera.up` reading the new value while orbiting still happens about the
 * OLD axis.
 *
 * Every assertion below is stated as an INVARIANT of the rotation (what an azimuthal
 * orbit about a given axis must preserve), never as magic coordinates — so the test
 * still means what it says if three changes how it names or stores the frame.
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { PerspectiveCamera } from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { syncOrbitUpAxis } from '../../viewport/controls';

/**
 * Deliberately OFF-axis.  An on-axis camera would sit at the orbit pole, where
 * `rotateLeft` is a no-op for a reason that has nothing to do with the defect — so an
 * on-axis fixture would pass whether or not the frame was synced.
 */
const START = { x: 5, y: 0, z: 3 };
/** Orbit radius in the plane normal to the NEW (+Z) up axis. */
const RADIUS_XY = Math.hypot(START.x, START.y);
const AZIMUTH = 0.4;

function makeControls(): { camera: PerspectiveCamera; controls: OrbitControls } {
  // Constructed with the three default up (0,1,0), matching a camera that was built
  // before anything set a different up — which is when the stale frame is captured.
  const camera = new PerspectiveCamera(60, 800 / 600, 0.1, 1000);
  camera.position.set(START.x, START.y, START.z);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld();

  const domElement = document.createElement('canvas');
  Object.defineProperty(domElement, 'clientHeight', { value: 600 });
  Object.defineProperty(domElement, 'clientWidth', { value: 800 });

  const controls = new OrbitControls(camera, domElement);
  // Damping would spread one rotation over many frames; without it a single update()
  // consumes the whole delta, so the assertions are deterministic.
  controls.enableDamping = false;
  controls.target.set(0, 0, 0);
  controls.update();

  return { camera, controls };
}

describe('syncOrbitUpAxis', () => {
  let camera: PerspectiveCamera;
  let controls: OrbitControls;

  beforeEach(() => {
    ({ camera, controls } = makeControls());
  });

  it('re-derives the orbit frame so the camera orbits about the NEW up axis', () => {
    camera.up.set(0, 0, 1);
    syncOrbitUpAxis(controls);
    controls.update();

    controls.rotateLeft(AZIMUTH);
    controls.update();

    // An azimuthal rotation about +Z preserves the component ALONG +Z…
    expect(camera.position.z).toBeCloseTo(START.z, 6);
    // …and the orbit radius in the plane NORMAL to it.
    expect(Math.hypot(camera.position.x, camera.position.y)).toBeCloseTo(RADIUS_XY, 6);
    // Guard against a vacuous pass: the rotation has to have actually happened.
    expect(camera.position.x).not.toBeCloseTo(START.x, 3);
  });

  it('without the sync, the same sequence still orbits about the OLD +Y axis', () => {
    // The contrast case, documenting the bug rather than merely asserting the fix.
    camera.up.set(0, 0, 1);
    controls.update();

    controls.rotateLeft(AZIMUTH);
    controls.update();

    // Conserved quantities of a rotation about the STALE +Y frame, not the new +Z one.
    expect(camera.position.y).toBeCloseTo(START.y, 6);
    expect(Math.hypot(camera.position.x, camera.position.z)).toBeCloseTo(
      Math.hypot(START.x, START.z),
      6,
    );
    // The tell: z moves, when an orbit about +Z would have held it fixed.
    expect(camera.position.z).not.toBeCloseTo(START.z, 3);
  });

  it('degrades to a no-op on a controls-like object with no orbit frame', () => {
    // Hand-rolled stubs in debugBridge.test.tsx have no _quat, and a future three release
    // could rename it.  Either must degrade quietly rather than throw inside the debug
    // bridge — the BEHAVIOURAL tests above are what would catch a rename re-hiding the bug.
    const stub = { object: { up: { x: 0, y: 0, z: 1 } } };
    expect(() => syncOrbitUpAxis(stub as unknown as OrbitControls)).not.toThrow();
  });
});
