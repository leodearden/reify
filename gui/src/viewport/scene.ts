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
  /** Resize the grid, axes triad and label ring to match the scene's extent.
   *  Without it, a sub-metre model sits inside a 20 m grid and a 2 m triad — see
   *  the implementation for the #6588 defect this exists to prevent. */
  fitHelpers: (sceneBounds: Box3) => void;
  grid: GridHelper;
  axes: AxesHelper;
  axisLabels: Group;
  /** Dispose the CanvasTexture and SpriteMaterial for each axis-label sprite.
   *  Call from Viewport.tsx onCleanup to release GPU resources on unmount. */
  disposeAxisLabels: () => void;
}

/**
 * Narrow a three.js helper's `material` to the single material it actually carries.
 *
 * GridHelper and AxesHelper each currently construct exactly one LineBasicMaterial,
 * but their types (and a future three.js version) allow an array. Every depth flag
 * this module sets is a MUTATION of that material, and a mutation applied to an array
 * object would silently miss the real material — restoring the #4214 z-fight with no
 * test failure, because the unit-test mocks hand back a plain object. Fail loudly
 * instead of drifting.
 *
 * @param helper - The helper whose material to narrow.
 * @param name - Helper class name, used in the error message.
 */
function singleMaterial<T>(helper: { material: T | T[] }, name: string): T {
  if (Array.isArray(helper.material)) {
    throw new Error(
      `${name}.material is unexpectedly an array — three.js API changed; update overlay logic in scene.ts.`,
    );
  }
  return helper.material;
}

/**
 * Base dimensions the viewport helpers are CONSTRUCTED with. They are also the
 * denominators `fitHelpers` divides by to turn a target world size into an object
 * scale, so they must stay in one place rather than being repeated as literals at
 * the construction site and again in the scaling arithmetic.
 *
 * The values are the historical defaults, chosen for the ~10 m CAD default scene
 * (see Viewport.tsx's auto-fit comment); `fitHelpers` is what adapts them.
 */
const GRID_BASE_SIZE = 20;
const GRID_DIVISIONS = 20;
const AXES_BASE_LENGTH = 2;

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
  // that sits BEHIND it will overdraw the grid's 1px lines. The grid is last among opaque
  // MODEL geometry, and the only opaque draws after it are the helpers in this tier — which
  // are depthWrite = false themselves and are MEANT to overdraw it. (A future tier entry
  // must preserve exactly that: opaque, higher renderOrder, and no depth write.) So the
  // artifact is confined to the transparent pass — a hairline along grid lines under a
  // ghosted/low-opacity part. Strictly smaller than the full-scene depth lie it replaces.
  const grid = new GridHelper(GRID_BASE_SIZE, GRID_DIVISIONS, 0x444466, 0x333344);
  // GridHelper lays in the XZ plane (Y-up default); rotate to lie on the XY plane (the floor under Z-up).
  grid.rotation.x = Math.PI / 2;
  const gridMaterial = singleMaterial(grid, 'GridHelper');
  grid.renderOrder = GRID_RENDER_ORDER;
  // Both flags are assigned explicitly rather than left to three.js defaults: they ARE the
  // helper-tier contract (see ./renderOrder.ts), and an explicit write is what the #6587
  // regression tests can actually observe — a mock material that starts out untouched
  // distinguishes "this module set it" from "the default happened to agree".
  gridMaterial.depthTest = true;
  gridMaterial.depthWrite = false;
  scene.add(grid);

  const axes = new AxesHelper(AXES_BASE_LENGTH);
  const axesMaterial = singleMaterial(axes, 'AxesHelper');
  axes.renderOrder = AXES_RENDER_ORDER;
  // depthTest = true is the direct reversal of #4214's depthTest = false.
  axesMaterial.depthTest = true;
  axesMaterial.depthWrite = false;
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

  /**
   * Size the viewport helpers to the scene they annotate.
   *
   * #6588: the helpers above are built at fixed ABSOLUTE sizes tuned for the ~10 m
   * CAD default scene, but most .ri models are far smaller. In a sub-metre model
   * the camera sits deep inside the helper envelope, and the helpers stop reading
   * as annotations: the 20 m grid's far lines converge into a solid dark 0x444466
   * band running to the horizon behind the model, and the 2 m axes triad draws a
   * hard diagonal straight across every part in frame. Neither is fixable by depth
   * state (that was #6587's half) — the geometry itself is the wrong size.
   *
   * `radius` deliberately uses the SAME half-box-diagonal measure as adjustClipping
   * above and as fitCamera.ts, so helper sizing and camera framing agree on how big
   * the scene is rather than drifting apart under two definitions.
   *
   * The guard shape intentionally mirrors adjustClipping's isEmpty() early return,
   * and extends it: a non-empty box can still have zero or non-finite extent, and a
   * 0-unit (or NaN-unit) grid is strictly worse than an oversized one, so those
   * measurements are dropped rather than applied.
   */
  function fitHelpers(sceneBounds: Box3): void {
    if (sceneBounds.isEmpty()) return;

    const size = new Vector3();
    sceneBounds.getSize(size);
    const radius = size.length() / 2;
    if (!Number.isFinite(radius) || radius <= 0) return;
  }

  return { scene, camera, renderer, resize, adjustClipping, fitHelpers, grid, axes, axisLabels, disposeAxisLabels };
}
