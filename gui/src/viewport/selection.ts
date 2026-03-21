import { Raycaster, Vector2 } from 'three';
import type { Scene, PerspectiveCamera, Mesh } from 'three';

export interface SelectionOptions {
  scene: Scene;
  camera: PerspectiveCamera;
  domElement: HTMLElement;
  getMeshes: () => Map<string, Mesh>;
  onHover: (path: string | null) => void;
  onSelect: (path: string | null) => void;
}

export interface SelectionContext {
  setHovered: (path: string | null) => void;
  setSelected: (path: string | null) => void;
  fitToView: () => void;
  dispose: () => void;
}

/**
 * Creates a selection system for pointer-based hover/click detection
 * on meshes managed by meshManager.
 */
export function createSelection(options: SelectionOptions): SelectionContext {
  const { camera, domElement, getMeshes, onHover } = options;
  const raycaster = new Raycaster();
  const ndc = new Vector2();

  function computeNDC(event: MouseEvent): void {
    const rect = domElement.getBoundingClientRect();
    ndc.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    ndc.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  }

  function raycast(event: MouseEvent): string | null {
    computeNDC(event);
    raycaster.setFromCamera(ndc, camera);
    const meshes = Array.from(getMeshes().values());
    const intersections = raycaster.intersectObjects(meshes);
    if (intersections.length > 0) {
      return intersections[0].object.name;
    }
    return null;
  }

  function handlePointerMove(event: Event): void {
    const entityPath = raycast(event as MouseEvent);
    onHover(entityPath);
  }

  domElement.addEventListener('pointermove', handlePointerMove);

  function setHovered(_path: string | null): void {
    // stub
  }

  function setSelected(_path: string | null): void {
    // stub
  }

  function fitToView(): void {
    // stub
  }

  function dispose(): void {
    // stub
  }

  return { setHovered, setSelected, fitToView, dispose };
}
