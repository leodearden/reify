import {
  Raycaster,
  Vector2,
  Color,
  WireframeGeometry,
  LineSegments,
  LineBasicMaterial,
} from 'three';
import type { Scene, PerspectiveCamera, Mesh, MeshStandardMaterial } from 'three';
import { THEME_TOKENS } from '../theme';

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
/** Emissive highlight color derived from theme accent. */
const HIGHLIGHT_COLOR = THEME_TOKENS.accent;

export function createSelection(options: SelectionOptions): SelectionContext {
  const { scene, camera, domElement, getMeshes, onHover, onSelect } = options;
  const raycaster = new Raycaster();
  const ndc = new Vector2();
  let previousHoveredPath: string | null = null;
  let currentWireframe: LineSegments | null = null;

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

  function handlePointerDown(event: Event): void {
    const entityPath = raycast(event as MouseEvent);
    onSelect(entityPath);
  }

  domElement.addEventListener('pointermove', handlePointerMove);
  domElement.addEventListener('pointerdown', handlePointerDown);

  function setHovered(path: string | null): void {
    const meshes = getMeshes();

    // Reset previous hover
    if (previousHoveredPath !== null) {
      const prevMesh = meshes.get(previousHoveredPath);
      if (prevMesh) {
        (prevMesh.material as MeshStandardMaterial).emissive.set(0x000000);
      }
    }

    // Apply new hover
    if (path !== null) {
      const mesh = meshes.get(path);
      if (mesh) {
        (mesh.material as MeshStandardMaterial).emissive.set(HIGHLIGHT_COLOR);
        previousHoveredPath = path;
      } else {
        previousHoveredPath = null;
      }
    } else {
      previousHoveredPath = null;
    }
  }

  function removeWireframe(): void {
    if (currentWireframe) {
      scene.remove(currentWireframe);
      currentWireframe.geometry.dispose();
      currentWireframe = null;
    }
  }

  function setSelected(path: string | null): void {
    // Remove existing wireframe
    removeWireframe();

    if (path === null) return;

    const mesh = getMeshes().get(path);
    if (!mesh) return;

    // Create wireframe overlay
    const wireGeom = new WireframeGeometry(mesh.geometry);
    const wireMat = new LineBasicMaterial({ color: HIGHLIGHT_COLOR });
    currentWireframe = new LineSegments(wireGeom, wireMat);
    scene.add(currentWireframe);
  }

  function fitToView(): void {
    // stub
  }

  function dispose(): void {
    // stub
  }

  return { setHovered, setSelected, fitToView, dispose };
}
