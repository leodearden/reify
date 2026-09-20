import type { PerspectiveCamera } from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { ORBIT_MIN_DISTANCE_FLOOR, ORBIT_MAX_DISTANCE } from './orbitDistance';

export interface ControlsContext {
  controls: OrbitControls;
  update: () => void;
  dispose: () => void;
}

/**
 * Creates an OrbitControls wrapper with sensible defaults.
 *
 * `minDistance` is seeded to the absolute policy FLOOR rather than to a guessed
 * absolute distance, because this runs in `Viewport.tsx`'s `onMount` BEFORE any
 * geometry exists — there are no bounds here to derive a model-appropriate floor
 * from.  Seeding the floor means the pre-geometry state blocks no commanded pose;
 * `fitCameraToBox` then tightens it to the model scale as soon as anything is
 * framed.  The previous guess of 0.5 m did block poses: a fitted 75 mm part sits
 * at ~86 mm, so the clamp silently relocated the camera ~6× too far out (#6496).
 *
 * `maxDistance` is unchanged in value — it is not implicated by that defect and is
 * only re-homed into `orbitDistance.ts` so both limits are stated in one place.
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
  controls.minDistance = ORBIT_MIN_DISTANCE_FLOOR;
  controls.maxDistance = ORBIT_MAX_DISTANCE;

  return {
    controls,
    update: () => controls.update(),
    dispose: () => controls.dispose(),
  };
}
