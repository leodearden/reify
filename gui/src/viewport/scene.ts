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
import type { Box3, Group } from 'three';
import { THEME_TOKENS } from '../theme';
import { createAxisLabels } from './axisLabels';
import { GRID_RENDER_ORDER, AXES_RENDER_ORDER } from './renderOrder';

export interface SceneContext {
  scene: Scene;
  camera: PerspectiveCamera;
  renderer: WebGLRenderer;
  resize: (width: number, height: number) => void;
  adjustClipping: (sceneBounds: Box3) => void;
  grid: GridHelper;
  axes: AxesHelper;
  axisLabels: Group;
  /** Dispose the CanvasTexture and SpriteMaterial for each axis-label sprite.
   *  Call from Viewport.tsx onCleanup to release GPU resources on unmount. */
  disposeAxisLabels: () => void;
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
  //
  // Viewport helpers (grid, axes, axis labels) all follow one rule, ordered by the
  // ladder in ./renderOrder.ts:
  //
  //   depthTest  = true   — helpers are full-scene-scale WORLD geometry, not a HUD, so
  //                         model geometry in front of them must occlude them (#6587).
  //   depthWrite = false  — a helper contributes nothing to the depth buffer, so it can
  //                         never occlude another helper drawn after it.
  //
  // Why that combination fixes the coplanar z-fight (#4214) without lying about depth:
  // GridHelper's two CENTRE lines lie on its local X/Z axes, and the π/2 rotation below
  // makes them exactly collinear with the AxesHelper's red (X) / green (Y) segments. Two
  // collinear primitives built from different vertex data and model matrices produce depth
  // values that differ only by float error, so under LESS_EQUAL the grey grid line
  // sometimes won and occluded the axis vector.
  //
  // #4214 broke that tie with depthTest = false on the axes, which wins by disabling depth
  // ENTIRELY — and therefore also made the axes (and the labels that copied the flag) beat
  // every solid mesh in the scene, drawing straight through parts that enclose or sit in
  // front of the origin. That is the #6587 defect, and this block reverses it.
  //
  // The tie is now broken by draw ORDER instead: three.js sorts by renderOrder BEFORE z in
  // both painterSortStable and reversePainterSortStable, so the grid draws first and writes
  // no depth, meaning the axes drawn after it can never fail their LEQUAL test against it —
  // #4214 stays fixed at every zoom level — while real geometry, which DID write depth,
  // still occludes both.
  //
  // ACCEPTED COSMETIC TRADE-OFF of grid depthWrite = false: an object drawn AFTER the grid
  // that sits BEHIND it will overdraw the grid's 1px lines. Opaque geometry cannot hit this
  // (the grid is last in the opaque pass at GRID_RENDER_ORDER), so it is limited to the
  // transparent pass — a hairline artifact along grid lines under a ghosted/low-opacity
  // part. Strictly smaller than the full-scene depth lie it replaces.
  const grid = new GridHelper(20, 20, 0x444466, 0x333344);
  // GridHelper lays in the XZ plane (Y-up default); rotate to lie on the XY plane (the floor under Z-up).
  grid.rotation.x = Math.PI / 2;
  // GridHelper currently always constructs a single LineBasicMaterial. Guard against a
  // future three.js version returning a material array: the depthWrite mutation below would
  // silently write to the array object instead of the material, restoring z-fighting with no
  // test failure (the unit-test mock uses a plain object, not an array, so it cannot catch
  // that regression).
  if (Array.isArray(grid.material)) {
    throw new Error(
      'GridHelper.material is unexpectedly an array — three.js API changed; update overlay logic in scene.ts.',
    );
  }
  grid.renderOrder = GRID_RENDER_ORDER;
  // depthTest is left at its default (true) — real meshes in front of the grid occlude it.
  grid.material.depthWrite = false;
  scene.add(grid);

  const axes = new AxesHelper(2);
  // Same guard for AxesHelper (added by #4214), for the same reason.
  if (Array.isArray(axes.material)) {
    throw new Error(
      'AxesHelper.material is unexpectedly an array — three.js API changed; update overlay logic in scene.ts.',
    );
  }
  axes.renderOrder = AXES_RENDER_ORDER;
  // Set explicitly rather than relying on the default: this line is the direct reversal of
  // #4214's depthTest = false, and the assignment is what the #6587 regression test pins.
  axes.material.depthTest = true;
  axes.material.depthWrite = false;
  scene.add(axes);

  const { group: axisLabels, dispose: disposeAxisLabels } = createAxisLabels();
  scene.add(axisLabels);

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

  return { scene, camera, renderer, resize, adjustClipping, grid, axes, axisLabels, disposeAxisLabels };
}
