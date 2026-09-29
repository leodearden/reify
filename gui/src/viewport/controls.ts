import { Vector3 } from 'three';
import type { PerspectiveCamera } from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { orbitFloorFor, ORBIT_MAX_DISTANCE } from './orbitDistance';

export interface ControlsContext {
  controls: OrbitControls;
  update: () => void;
  dispose: () => void;
}

/**
 * Creates an OrbitControls wrapper with sensible defaults.
 *
 * `minDistance` is seeded by applying the orbit-floor policy to the camera's startup
 * orbit distance: this runs before any geometry exists, and the first framing
 * (`fitCameraToBox`) replaces it.  See docs/debug-mcp-contract.md §6 point 3.
 *
 * @param camera - The camera to orbit.
 * @param domElement - The DOM element for pointer events.
 */
export function createControls(
  camera: PerspectiveCamera,
  domElement: HTMLElement,
): ControlsContext {
  const controls = new OrbitControls(camera, domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.1;
  controls.minDistance = orbitFloorFor(camera.position.distanceTo(controls.target));
  controls.maxDistance = ORBIT_MAX_DISTANCE;

  return {
    controls,
    update: () => controls.update(),
    dispose: () => controls.dispose(),
  };
}

/**
 * Re-derive OrbitControls' orbit frame from its camera's CURRENT `up` vector.
 *
 * This is the one place in the codebase that touches a three private field, so the
 * evidence for why there is no alternative lives here.  In three 0.183.2:
 *   • `_quat` is computed ONCE, in the constructor, as
 *     `setFromUnitVectors(object.up, (0,1,0))` (OrbitControls.js:405-407), with
 *     `_quatInverse` its inverse;
 *   • it is ASSIGNED only at line 406 and READ only at lines 695 and 784 — the rotate
 *     into and out of the orbit frame;
 *   • neither `update()` nor `reset()` re-derives it, and no public API does.
 * So mutating `camera.up` after construction leaves `camera.up` reading the new value
 * while orbiting still happens about the OLD axis (#6497).  The only other remedy would
 * be to tear down and reconstruct the OrbitControls on every up change, which would
 * discard its registered listeners (`change` → requestRender, `end` → persistCamera in
 * Viewport.tsx) and all interaction state.
 *
 * The presence check makes a controls-like stub, or a future three release that renames
 * these fields, degrade to a no-op instead of throwing inside the debug bridge.  That
 * guard must never be allowed to silently re-hide the defect: the test that catches a
 * rename is the BEHAVIOURAL one in `orbitUpAxis.test.ts`, which asserts which axis the
 * camera actually orbits about, not which fields exist.
 */
export function syncOrbitUpAxis(controls: OrbitControls): void {
  const frame = controls as unknown as {
    _quat?: { setFromUnitVectors?: (a: Vector3, b: Vector3) => void };
    _quatInverse?: { copy?: (q: unknown) => { invert: () => void } };
  };
  if (typeof frame._quat?.setFromUnitVectors !== 'function') return;
  if (typeof frame._quatInverse?.copy !== 'function') return;

  frame._quat.setFromUnitVectors(controls.object.up, new Vector3(0, 1, 0));
  frame._quatInverse.copy(frame._quat).invert();
}
