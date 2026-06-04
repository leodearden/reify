/**
 * Per-viewport selection system. Each viewport (`<Viewport>`) constructs its own
 * `createSelection` instance bound to its own scene, camera, and domElement.
 *
 * Cross-viewport selection synchronisation is handled externally through the shared
 * `selectionStore` (not through this module) — `createSelection` only manages the
 * Three.js raycasting, hover highlight, and wireframe overlay for one scene.
 *
 * `raycaster.firstHitOnly = true` keeps per-viewport raycasting O(log n) via the
 * BVH acceleration patch applied to `Mesh.prototype.raycast` below.
 */
import {
  Raycaster,
  Vector2,
  Box3,
  Color,
  EdgesGeometry,
  LineSegments,
  LineBasicMaterial,
  Mesh,
} from 'three';
import type { Scene, PerspectiveCamera, MeshStandardMaterial } from 'three';
import { acceleratedRaycast } from 'three-mesh-bvh';
import { THEME_TOKENS } from '../theme';
import { fitCameraToBox } from './fitCamera';

// Patch Mesh prototype for BVH-accelerated raycasting
Mesh.prototype.raycast = acceleratedRaycast;

export interface SelectionModifiers {
  ctrl: boolean;
  shift: boolean;
}

export interface SelectionOptions {
  scene: Scene;
  camera: PerspectiveCamera;
  domElement: HTMLElement;
  getMeshes: () => Map<string, Mesh>;
  onHover: (path: string | null) => void;
  onSelect: (path: string | null, modifiers: SelectionModifiers) => void;
  controls?: { target: { copy: (v: any) => void } };
}

export interface SelectionContext {
  setHovered: (path: string | null) => void;
  setSelected: (paths: Iterable<string> | string | null) => void;
  refreshSelected: () => void;
  fitToView: () => void;
  flyToEntity: (entityPath: string) => void;
  invalidateRect: () => void;
  dispose: () => void;
}

/**
 * Creates a selection system for pointer-based hover/click detection
 * on meshes managed by meshManager.
 *
 * Color semantics:
 *   HIGHLIGHT_COLOR (accent blue)  — hover emissive, applied via setHovered.
 *   SELECTION_COLOR (orange)       — selection wireframe outline, applied via addWireframeFor.
 * Keeping them distinct ensures a selected mesh and a hovered mesh are never
 * visually identical (the original "subtle color changes" complaint).
 */
/** Emissive highlight color derived from theme accent (used for hover). */
const HIGHLIGHT_COLOR = THEME_TOKENS.accent;
/** High-contrast wireframe color for the selection overlay — distinct from hover. */
const SELECTION_COLOR = THEME_TOKENS.selection;

export function createSelection(options: SelectionOptions): SelectionContext {
  const { scene, camera, domElement, getMeshes, onHover, onSelect, controls } = options;
  const raycaster = new Raycaster();
  (raycaster as any).firstHitOnly = true;
  const ndc = new Vector2();
  let previousHoveredPath: string | null = null;
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
  let refreshRafPending = false;
  let refreshRafId = 0;

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
      // Read modifiers from the pointerup event (up-time state, since click-intent is confirmed on up)
      const modifiers: SelectionModifiers = { ctrl: me.ctrlKey, shift: me.shiftKey };
      onSelect(entityPath, modifiers);
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

  /** Map from entity path → its wireframe LineSegments currently in the scene. */
  const wireframes = new Map<string, LineSegments>();

  function removeWireframeFor(path: string): void {
    const wf = wireframes.get(path);
    if (wf) {
      scene.remove(wf);
      wf.geometry.dispose();
      (wf.material as LineBasicMaterial).dispose();
      wireframes.delete(path);
    }
  }

  function addWireframeFor(path: string): void {
    const mesh = getMeshes().get(path);
    if (!mesh) return;
    const wireGeom = new EdgesGeometry(mesh.geometry);
    const wireMat = new LineBasicMaterial({ color: SELECTION_COLOR });
    const wf = new LineSegments(wireGeom, wireMat);
    scene.add(wf);
    wireframes.set(path, wf);
  }

  function setSelected(paths: Iterable<string> | string | null): void {
    // Normalize input to a Set<string>
    let targetSet: Set<string>;
    if (paths === null) {
      targetSet = new Set();
    } else if (typeof paths === 'string') {
      targetSet = new Set([paths]);
    } else {
      targetSet = new Set(paths);
    }

    // Remove wireframes for paths no longer in target
    for (const path of Array.from(wireframes.keys())) {
      if (!targetSet.has(path)) {
        removeWireframeFor(path);
      }
    }

    // Add wireframes for new paths
    for (const path of targetSet) {
      if (!wireframes.has(path)) {
        addWireframeFor(path);
      }
    }
  }

  function refreshSelected(): void {
    if (wireframes.size === 0) return;
    // Coalesce rapid calls (e.g. from meshManager sync ticks) into a single
    // rebuild per animation frame so we don't churn GPU buffers on every tick
    // when N entities are selected.
    if (!refreshRafPending) {
      refreshRafPending = true;
      refreshRafId = requestAnimationFrame(() => {
        refreshRafPending = false;
        refreshRafId = 0;
        if (isDisposed) return;
        // Rebuild all active wireframes (e.g. after geometry update)
        const paths = Array.from(wireframes.keys());
        for (const path of paths) {
          removeWireframeFor(path);
        }
        for (const path of paths) {
          addWireframeFor(path);
        }
      });
    }
  }

  function fitToView(): void {
    // getMeshes() delegates to getSceneMeshes(), intentionally excluding ghost meshes.
    // Ghost entities are secondary context (intermediate let-binding visualizations),
    // not primary framing targets — fitting the camera to them would be distracting.
    // Note: adjustClipping in Viewport.tsx *does* include ghosts via getGhostMeshes()
    // because clipping planes must encompass all visible geometry.
    const meshes = getMeshes();
    if (meshes.size === 0) return;

    const box = new Box3();
    for (const mesh of meshes.values()) {
      box.expandByObject(mesh);
    }

    if (box.isEmpty()) return;

    fitCameraToBox(camera, box, controls ? { controls } : undefined);
  }

  function flyToEntity(entityPath: string): void {
    const mesh = getMeshes().get(entityPath);
    if (!mesh) return;

    const box = new Box3();
    box.expandByObject(mesh);

    if (box.isEmpty()) return;

    fitCameraToBox(camera, box, controls ? { controls } : undefined);
  }

  function dispose(): void {
    isDisposed = true;
    if (hoverRafPending) {
      cancelAnimationFrame(hoverRafId);
      hoverRafPending = false;
      hoverRafId = 0;
    }
    if (refreshRafPending) {
      cancelAnimationFrame(refreshRafId);
      refreshRafPending = false;
      refreshRafId = 0;
    }
    domElement.removeEventListener('pointermove', handlePointerMove);
    domElement.removeEventListener('pointerdown', handlePointerDown);
    domElement.removeEventListener('pointerup', handlePointerUp);
    pointerDownPos = null;
    // Remove all active wireframes
    for (const path of Array.from(wireframes.keys())) {
      removeWireframeFor(path);
    }
  }

  return { setHovered, setSelected, refreshSelected, fitToView, flyToEntity, invalidateRect, dispose };
}
