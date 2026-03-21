import {
  Raycaster,
  Vector2,
  Vector3,
  Box3,
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
  controls?: { target: { copy: (v: any) => void } };
}

export interface SelectionContext {
  setHovered: (path: string | null) => void;
  setSelected: (path: string | null) => void;
  fitToView: () => void;
  flyToEntity: (entityPath: string) => void;
  invalidateRect: () => void;
  dispose: () => void;
}

/**
 * Creates a selection system for pointer-based hover/click detection
 * on meshes managed by meshManager.
 */
/** Emissive highlight color derived from theme accent. */
const HIGHLIGHT_COLOR = THEME_TOKENS.accent;

export function createSelection(options: SelectionOptions): SelectionContext {
  const { scene, camera, domElement, getMeshes, onHover, onSelect, controls } = options;
  const raycaster = new Raycaster();
  const ndc = new Vector2();
  let previousHoveredPath: string | null = null;
  let currentWireframe: LineSegments | null = null;
  let cachedRect: DOMRect | null = null;

  function getRect(): DOMRect {
    if (cachedRect === null) {
      cachedRect = domElement.getBoundingClientRect();
    }
    return cachedRect;
  }

  function invalidateRect(): void {
    cachedRect = null;
  }

  function computeNDC(event: MouseEvent): void {
    const rect = getRect();
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

  let pendingMoveEvent: MouseEvent | null = null;
  let hoverRafPending = false;
  let hoverRafId = 0;

  function processHover(): void {
    hoverRafPending = false;
    if (pendingMoveEvent) {
      const event = pendingMoveEvent;
      pendingMoveEvent = null;
      const entityPath = raycast(event);
      onHover(entityPath);
    }
  }

  function handlePointerMove(event: Event): void {
    pendingMoveEvent = event as MouseEvent;
    if (!hoverRafPending) {
      hoverRafPending = true;
      hoverRafId = requestAnimationFrame(processHover);
    }
  }

  const CLICK_THRESHOLD = 5; // px — below this is a click, above is a drag
  let pointerDownPos: { x: number; y: number } | null = null;
  let isDisposed = false;

  function handlePointerUp(event: Event): void {
    const me = event as MouseEvent;
    if (pointerDownPos === null) return;
    const dx = me.clientX - pointerDownPos.x;
    const dy = me.clientY - pointerDownPos.y;
    const distance = Math.sqrt(dx * dx + dy * dy);
    pointerDownPos = null;
    if (distance < CLICK_THRESHOLD) {
      const entityPath = raycast(me);
      onSelect(entityPath);
    }
  }

  function handlePointerDown(event: Event): void {
    const me = event as MouseEvent;
    pointerDownPos = { x: me.clientX, y: me.clientY };
  }

  domElement.addEventListener('pointermove', handlePointerMove);
  domElement.addEventListener('pointerdown', handlePointerDown);
  domElement.addEventListener('pointerup', handlePointerUp);

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
      (currentWireframe.material as LineBasicMaterial).dispose();
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
    const meshes = getMeshes();
    if (meshes.size === 0) return;

    const box = new Box3();
    for (const mesh of meshes.values()) {
      box.expandByObject(mesh);
    }

    if (box.isEmpty()) return;

    const center = new Vector3();
    const size = new Vector3();
    box.getCenter(center);
    box.getSize(size);

    const maxDim = Math.max(size.x, size.y, size.z);
    const fovRad = (camera.fov / 2) * (Math.PI / 180);
    const distance = maxDim / (2 * Math.tan(fovRad));

    const viewDir = new Vector3();
    camera.getWorldDirection(viewDir);
    // Position camera at center - viewDir * distance (backing away from center along view direction)
    camera.position.copy(center).sub(viewDir.multiplyScalar(distance));
    camera.lookAt(center);
    camera.updateProjectionMatrix();
    if (controls) {
      controls.target.copy(center);
    }
  }

  function flyToEntity(entityPath: string): void {
    const mesh = getMeshes().get(entityPath);
    if (!mesh) return;

    const box = new Box3();
    box.expandByObject(mesh);

    if (box.isEmpty()) return;

    const center = new Vector3();
    const size = new Vector3();
    box.getCenter(center);
    box.getSize(size);

    const maxDim = Math.max(size.x, size.y, size.z);
    const fovRad = (camera.fov / 2) * (Math.PI / 180);
    const distance = maxDim / (2 * Math.tan(fovRad));

    camera.position.copy(center);
    camera.position.z += distance;
    camera.lookAt(center);
    camera.updateProjectionMatrix();
  }

  function dispose(): void {
    isDisposed = true;
    if (hoverRafPending) {
      cancelAnimationFrame(hoverRafId);
      hoverRafPending = false;
      hoverRafId = 0;
    }
    domElement.removeEventListener('pointermove', handlePointerMove);
    domElement.removeEventListener('pointerdown', handlePointerDown);
    domElement.removeEventListener('pointerup', handlePointerUp);
    pointerDownPos = null;
    removeWireframe();
  }

  return { setHovered, setSelected, fitToView, flyToEntity, invalidateRect, dispose };
}
