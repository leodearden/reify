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
import type { Box3, Material } from 'three';
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
  // (5, 5, 5) is intentional under Z-up: z=5 places the camera above the XY ground plane
  // and the position gives a usable iso-ish view. A strict CAD iso would be ~(1,-1,1)*d but
  // the symmetric default is sufficient for first-launch framing.
  camera.position.set(5, 5, 5);

  // Renderer
  // preserveDrawingBuffer: html-to-image samples the canvas async after compositing;
  // without this the browser may invalidate the GL back-buffer between render() and read.
  // Accepted trade-off: small steady-state GPU fill-rate overhead (extra back-buffer copy) is
  // preferable to the complexity of toggling the flag per-session (context-creation attribute,
  // not a runtime toggle). The overhead is negligible for Reify's scene complexity.
  const renderer = new WebGLRenderer({ antialias: true, canvas, preserveDrawingBuffer: true });
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
  // GridHelper lays in the XZ plane (Y-up default); rotate to lie on the XY plane (the floor under Z-up).
  grid.rotation.x = Math.PI / 2;
  scene.add(grid);

  const axes = new AxesHelper(2);
  // The AxesHelper is coplanar with the XY GridHelper (both lie in the Z=0 plane). Without
  // intervention, floating-point depth jitter at some zoom levels causes the grey grid lines
  // to win the LESS_EQUAL depth test and occlude the red (X) / green (Y) axis vectors.
  //
  // Fix: make the axes a deliberate always-on-top origin gizmo — a conventional CAD affordance.
  // INTENTIONAL SIDE-EFFECT: depthTest=false means the axes render on top of ALL scene
  // objects, including solid model geometry that encloses or sits in front of the origin, not
  // just the coplanar grid. This is by design: always-visible origin axis gizmos are standard
  // in CAD viewports and the helper is only 2 units long, so the cosmetic trade-off is
  // acceptable and desirable (the gizmo is never accidentally hidden behind a model).
  //   renderOrder = 1  — draw axes AFTER the grid (grid keeps the default renderOrder 0)
  //   depthTest = false — axes fragments are never discarded; the gizmo is always on top
  //   depthWrite = false — axes do not pollute the depth buffer for subsequent draws
  // The cast to Material is safe: AxesHelper always constructs a single LineBasicMaterial.
  axes.renderOrder = 1;
  (axes.material as Material).depthTest = false;
  (axes.material as Material).depthWrite = false;
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
