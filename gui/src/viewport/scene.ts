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
import { createAxisLabels, DEFAULT_LABEL_OFFSET } from './axisLabels';
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
 * Snap `x` UP to the nearest 1 | 2 | 5 x 10^k.
 *
 * A grid is a measuring aid, so its cell must be a size a reader can hold in their
 * head: 0.1 m, 0.2 m, 0.5 m — never 0.10392... m. Snapping UP (never down) also
 * bounds the result: the worst case is a value just above 2, which snaps to 5, so
 * the returned spacing is at most 2.5x the requested one.
 *
 * @param x - Requested spacing; must be finite and > 0 (callers guard).
 */
function niceSpacing(x: number): number {
  const decade = Math.pow(10, Math.floor(Math.log10(x)));
  const m = x / decade; // in [1, 10)
  return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * decade;
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
 * How far past the axis tip the label ring sits, as a multiple of the axis length.
 *
 * Must be > 1 so the letter clears the tip rather than landing on the vector it
 * annotates. Kept small so the ring stays inside the scene's neighbourhood — the
 * #6588 defect was a ring parked at a fixed 2.3 world units, which for a sub-metre
 * model is several times the whole scene.
 */
const LABEL_TIP_MARGIN = 1.15;

/**
 * Vertical field of view, in degrees, of the viewport camera.
 *
 * Exported because it is NOT a private detail of the camera: axisLabels.ts's
 * LABEL_SCREEN_SCALE is calibrated against it. Under `sizeAttenuation: false` a
 * label covers `s * cot(fov/2) / 2` of the viewport height, so narrowing the fov
 * MAGNIFIES every label (fov 30 would take the current 4.8% to 9.2%) with nothing
 * in axisLabels.ts changing. Retuning the fov is a plausible unrelated change, so
 * scene.test.ts evaluates that formula against this constant and the sprites' real
 * scale — a fov move that pushes the labels back towards the #6588 footprint fails
 * there instead of silently shipping.
 */
export const CAMERA_FOV_DEG = 60;

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
  const camera = new PerspectiveCamera(CAMERA_FOV_DEG, width / height, 0.1, 10000);
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

  const {
    group: axisLabels,
    dispose: disposeAxisLabels,
    setOffset: setAxisLabelOffset,
  } = createAxisLabels();
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
   * DEGENERATE BOUNDS RESET, they do not preserve. A degenerate measurement — an
   * empty Box3, a zero-extent one, a NaN/Infinity one — is never applied (a 0-unit
   * or NaN-unit grid is strictly worse than an oversized one), but it also must not
   * leave the PREVIOUS scene's sizing standing. Closing or clearing a document takes
   * props.meshes to {}, which makes Viewport.tsx's mesh-sync effect hand this an
   * empty Box3; preserving would leave the now-empty viewport wearing the last
   * model's helper scale — a 2 m grid and a 0.3 m triad after a sub-metre part,
   * instead of the defaults an empty scene is tuned for. That is directly visible,
   * unlike adjustClipping's stale near/far planes, so this guard resets where
   * adjustClipping merely returns.
   */
  function resetHelpers(): void {
    // Scale 1 IS the construction sizing (GRID_BASE_SIZE / AXES_BASE_LENGTH), and
    // DEFAULT_LABEL_OFFSET is the ring position that pairs with it — it is the same
    // base case the good path would produce, since AXES_BASE_LENGTH * LABEL_TIP_MARGIN
    // (2 * 1.15) is exactly DEFAULT_LABEL_OFFSET (2.3).
    grid.scale.setScalar(1);
    axes.scale.setScalar(1);
    setAxisLabelOffset(DEFAULT_LABEL_OFFSET);
  }

  function fitHelpers(sceneBounds: Box3): void {
    // isEmpty() is checked BEFORE any measurement, mirroring adjustClipping's guard.
    if (sceneBounds.isEmpty()) {
      resetHelpers();
      return;
    }

    const size = new Vector3();
    sceneBounds.getSize(size);
    const radius = size.length() / 2;
    if (!Number.isFinite(radius) || radius <= 0) {
      resetHelpers();
      return;
    }

    // radius / 5 targets ~10 cells across the model's diameter: enough to read the
    // grid as a ruler, few enough that it does not extend far enough to converge
    // into a horizon band.
    const spacing = niceSpacing(radius / 5);
    const gridWorldSize = spacing * GRID_DIVISIONS;
    const axesWorldLength = spacing * 3;

    // A GridHelper/AxesHelper is a LineSegments, so a UNIFORM object scale rescales
    // the line SPACING along with the extent — no geometry rebuild, and no dispose()
    // of the old buffers. The division count stays GRID_DIVISIONS, so line density
    // (and therefore the aliasing budget) is scene-independent: the grid never gains
    // lines as the model shrinks.
    //
    // Worked cases:
    //   r = 0.3   -> 0.1 m cells, 2 m grid, 0.3 m axes. The sub-metre printer stops
    //                sitting inside a 20 m grid whose far lines converge into the
    //                reported dark 0x444466 horizon band, and the 2 m triad stops
    //                drawing a hard diagonal across every part in frame.
    //   r = 5     -> 1 m cells, 20 m grid — IDENTICAL to the pre-#6588 default, so
    //                the ~10 m CAD scene the helpers were tuned for is undisturbed.
    //   r = 0.005 -> 1 mm cells, 20 mm grid.
    grid.scale.setScalar(gridWorldSize / GRID_BASE_SIZE);
    axes.scale.setScalar(axesWorldLength / AXES_BASE_LENGTH);

    // The labels must follow the tip they annotate, and they must do so by MOVING,
    // not by scaling their Group: the r183 sprite shader multiplies on-screen size
    // by length(modelMatrix[0].xyz), which includes ancestor scale, so a Group scale
    // here would silently undo axisLabels.ts's constant-screen-size fix and
    // reproduce #6588 with nothing else changing. See createAxisLabels' setOffset
    // doc for the same constraint stated from the other end.
    setAxisLabelOffset(axesWorldLength * LABEL_TIP_MARGIN);

    // No requestRender() here: Viewport.tsx's mesh-sync effect already invalidates
    // after calling this, and re-rendering from inside a sizing helper would couple
    // this module to the render-on-demand loop it knows nothing about.
  }

  return { scene, camera, renderer, resize, adjustClipping, fitHelpers, grid, axes, axisLabels, disposeAxisLabels };
}
