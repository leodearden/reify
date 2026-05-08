import {
  Scene,
  PerspectiveCamera,
  WebGLRenderer,
  AmbientLight,
  DirectionalLight,
  GridHelper,
  AxesHelper,
  Color,
  Vector3,
} from 'three';
import type { Box3 } from 'three';
import { THEME_TOKENS } from '../theme';

export interface SceneContext {
  scene: Scene;
  camera: PerspectiveCamera;
  renderer: WebGLRenderer;
  resize: (width: number, height: number) => void;
  adjustClipping: (sceneBounds: Box3) => void;
  grid: GridHelper;
  axes: AxesHelper;
}

/**
 * Creates a Three.js scene with camera, renderer, lights, and helpers.
 * @param canvas - The HTML canvas element to render into.
 * @param width - Initial viewport width.
 * @param height - Initial viewport height.
 */
export function createScene(
  canvas: HTMLCanvasElement,
  width: number,
  height: number,
): SceneContext {
  const scene = new Scene();

  // Camera
  const camera = new PerspectiveCamera(60, width / height, 0.1, 10000);
  // Reify kernel is Z-up (XY ground plane, +Z extrusion direction). Set this BEFORE
  // OrbitControls is constructed in Viewport.tsx so its rotation basis is correct.
  camera.up.set(0, 0, 1);
  camera.position.set(5, 5, 5);

  // Renderer
  const renderer = new WebGLRenderer({ antialias: true, canvas });
  renderer.setPixelRatio(window.devicePixelRatio ?? 1);
  renderer.setSize(width, height);
  renderer.setClearColor(new Color(THEME_TOKENS.viewportBg), 1);

  // Lighting
  const ambient = new AmbientLight(0xffffff, 0.4);
  scene.add(ambient);

  const directional = new DirectionalLight(0xffffff, 0.8);
  directional.position.set(5, 10, 7);
  scene.add(directional);

  // Camera-following headlight — stays fixed relative to the camera
  const headlight = new DirectionalLight(0xffffff, 0.6);
  headlight.position.set(0, 0, 1);
  camera.add(headlight);
  scene.add(camera); // Camera must be in scene graph for its children to render

  // Helpers
  const grid = new GridHelper(20, 20, 0x444466, 0x333344);
  scene.add(grid);

  const axes = new AxesHelper(2);
  scene.add(axes);

  function resize(w: number, h: number) {
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setPixelRatio(window.devicePixelRatio ?? 1);
    renderer.setSize(w, h);
  }

  function adjustClipping(sceneBounds: Box3): void {
    if (sceneBounds.isEmpty()) return;

    const center = new Vector3();
    const size = new Vector3();
    sceneBounds.getCenter(center);
    sceneBounds.getSize(size);

    const dist = camera.position.distanceTo(center);
    const sceneRadius = size.length() / 2;
    const extent = dist + sceneRadius;

    camera.near = Math.max(extent * 0.001, 0.01);
    camera.far = Math.max(extent * 10, 100);
    camera.updateProjectionMatrix();
  }

  return { scene, camera, renderer, resize, adjustClipping, grid, axes };
}
