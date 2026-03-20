import type { PerspectiveCamera } from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

export interface ControlsContext {
  controls: OrbitControls;
  update: () => void;
  dispose: () => void;
}

/**
 * Creates an OrbitControls wrapper with sensible defaults.
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
  controls.minDistance = 0.5;
  controls.maxDistance = 500;

  return {
    controls,
    update: () => controls.update(),
    dispose: () => controls.dispose(),
  };
}
