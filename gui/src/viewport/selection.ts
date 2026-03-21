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
