import {
  CanvasTexture,
  Group,
  LinearFilter,
  Sprite,
  SpriteMaterial,
} from 'three';

import { AXIS_LABEL_RENDER_ORDER } from './renderOrder';

/**
 * INITIAL offset from origin where each axis label is placed.
 *
 * Must be > AxesHelper size (2) so labels sit just beyond the axis tip. This is the
 * offset in force before any scene bounds have arrived; once they have, scene.ts's
 * `fitHelpers` calls `setOffset` with a scene-scaled distance (see below).
 *
 * Exported because it is also the value scene.ts's `fitHelpers` restores when a
 * scene measurement is degenerate (an emptied document, a zero-extent Box3): the
 * helper sizing there returns to the UNSCALED grid/axes, so the ring must return to
 * the offset that pairs with them. Sharing this constant is what keeps "reset" and
 * "as constructed" the same position rather than two literals that can drift.
 */
export const DEFAULT_LABEL_OFFSET = 2.3;

/** Module-local alias, kept so the LABELS spec below reads as it always has. */
const LABEL_OFFSET = DEFAULT_LABEL_OFFSET;

/**
 * On-screen size of a label sprite, as a fraction of the viewport HEIGHT divided
 * by cot(fov/2)/2 — i.e. the `scale` the r183 sprite shader consumes when the
 * material sets `sizeAttenuation: false`.
 *
 * Derivation (three r183 `sprite.glsl.js`, verbatim):
 *
 *     vec4 mvPosition = modelViewMatrix[3];
 *     vec2 scale = vec2(length(modelMatrix[0].xyz), length(modelMatrix[1].xyz));
 *     #ifndef USE_SIZEATTENUATION
 *       if (isPerspective) scale *= -mvPosition.z;
 *     #endif
 *
 * That `-mvPosition.z` factor exactly cancels the perspective divide, so the
 * label's share of the viewport height is
 *
 *     f = s * cot(fov/2) / 2
 *
 * with NO camera-distance term. At the app's fov of 60 (see scene.ts),
 * 0.055 * 1.7320508 / 2 = 4.8% of the viewport height, at EVERY camera distance.
 *
 * WHY (#6588): with three.js's default `sizeAttenuation: true` the divide survives
 * and f = worldScale * cot(fov/2) / (2 * d). At the reported dogfood camera
 * (0.2923, -0.2809, 1.8260) the Z label at (0, 0, 2.3) is only d = 0.6237 away, so
 * the old LABEL_SCALE of 0.5 world units gave f = 0.694 — the glyph covered 69% of
 * the frame, magnified ~9x out of a 64-texel texture. That bilinear magnification
 * across ~9-px texel cells is the reported blocky stair-stepped band, and a
 * magnified "Z"/"Y" reads as a band terminating in a triangular wedge.
 */
const LABEL_SCREEN_SCALE = 0.055;

/**
 * Edge length, in texels, of the square offscreen canvas each glyph is drawn into.
 *
 * Sized against the WORST case rather than the typical one: LABEL_SCREEN_SCALE puts
 * the label at 4.8% of the viewport height, which on a 1600-device-pixel-tall HiDPI
 * viewport is ~77 device pixels — comfortably under 128, so the glyph is MINIFIED
 * (never magnified) on every display we target. The #6588 value of 64 was on the
 * wrong side of that line, and a magnified hard-edged glyph is exactly the reported
 * stair-stepping.
 */
const LABEL_TEXTURE_PX = 128;

/**
 * Glyph height as a fraction of LABEL_TEXTURE_PX. Held constant so raising the
 * texture resolution raises the LETTER's resolution rather than shrinking the
 * letter inside a larger, mostly-empty canvas.
 */
const LABEL_FONT_RATIO = 0.75;

interface LabelSpec {
  axis: 'X' | 'Y' | 'Z';
  color: number;
  position: [number, number, number];
}

const LABELS: LabelSpec[] = [
  { axis: 'X', color: 0xff0000, position: [LABEL_OFFSET, 0, 0] },
  { axis: 'Y', color: 0x00ff00, position: [0, LABEL_OFFSET, 0] },
  { axis: 'Z', color: 0x0000ff, position: [0, 0, LABEL_OFFSET] },
];

/**
 * Build a camera-facing sprite for a single axis letter.
 *
 * The glyph is drawn white onto the CanvasTexture; SpriteMaterial.color
 * applies the per-axis tint so the color is a first-class, inspectable
 * material property (testable without a real WebGL context).
 *
 * If `canvas.getContext('2d')` returns null (jsdom / headless), the guard
 * skips drawing but still produces a correctly-colored, positioned sprite.
 */
function makeTextSprite(spec: LabelSpec): Sprite {
  const canvas = document.createElement('canvas');
  canvas.width = LABEL_TEXTURE_PX;
  canvas.height = LABEL_TEXTURE_PX;

  const centre = LABEL_TEXTURE_PX / 2;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.clearRect(0, 0, LABEL_TEXTURE_PX, LABEL_TEXTURE_PX);
    ctx.fillStyle = '#ffffff';
    ctx.font = `bold ${Math.round(LABEL_TEXTURE_PX * LABEL_FONT_RATIO)}px sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(spec.axis, centre, centre);
  }

  const texture = new CanvasTexture(canvas);
  // CanvasTexture inherits minFilter = LinearMipmapLinearFilter. Now that
  // LABEL_SCREEN_SCALE fixes the label at ~4.8% of the frame, the glyph is usually
  // MINIFIED, so that default would resolve a 128-px letter through a blurred mip —
  // trading #6588's stair-stepping for mush. Sample the base level directly and skip
  // building the mip chain we would never want.
  texture.minFilter = LinearFilter;
  texture.generateMipmaps = false;
  const material = new SpriteMaterial({
    map: texture,
    color: spec.color,
    // Depth-tested: labels are world-space geometry at the axis tips, so model
    // geometry between them and the camera must occlude them (#6587). They still
    // write no depth — see the helper rule in ./renderOrder.ts.
    depthTest: true,
    depthWrite: false,
    transparent: true,
    // Constant screen size regardless of camera distance — see LABEL_SCREEN_SCALE
    // for the shader derivation and the #6588 repro this prevents.
    //
    // This does NOT weaken #6587's depth fix: `mvPosition = modelViewMatrix[3]` is
    // still the sprite's WORLD position, so the fragment depth (and hence occlusion
    // by model geometry in front of the label) is unchanged. A label behind the
    // camera yields w_clip < 0 and is near-plane clipped, not mirrored into frame.
    sizeAttenuation: false,
  });

  const sprite = new Sprite(material);
  sprite.name = `axis-label-${spec.axis}`;
  sprite.userData.axis = spec.axis;
  sprite.renderOrder = AXIS_LABEL_RENDER_ORDER;
  sprite.scale.set(LABEL_SCREEN_SCALE, LABEL_SCREEN_SCALE, 1);
  sprite.position.set(...spec.position);

  return sprite;
}

/**
 * Create a Group containing three camera-facing "X"/"Y"/"Z" text sprites
 * positioned just beyond the tips of the AxesHelper triad.
 *
 * Labels follow the uniform viewport-helper rule (see ./renderOrder.ts): they are
 * depth-TESTED, so a model surface between the camera and the label tip correctly
 * occludes them (#6587), and they never write depth. They sit at the top of the
 * helper tier (AXIS_LABEL_RENDER_ORDER), so they always draw over the axis they
 * annotate. The coplanar grid still cannot occlude them, because the grid writes
 * no depth either (see scene.ts, #4214).
 *
 * Labels are also CONSTANT SCREEN SIZE (`sizeAttenuation: false`), covering ~4.8%
 * of the viewport height at any camera distance — see LABEL_SCREEN_SCALE for the
 * shader derivation and the #6588 defect it prevents.
 *
 * COUPLING WARNING: the r183 sprite shader reads `length(modelMatrix[0].xyz)`, i.e.
 * the sprite's WORLD matrix, so an ANCESTOR scale multiplies the on-screen size and
 * silently re-breaks that invariant. The returned `group` must therefore never be
 * scaled. To follow a scene-sized axis triad, reposition the sprites via `setOffset`
 * instead (scene.ts's `fitHelpers` does exactly this).
 *
 * Visibility should be driven by the same signal that controls the axes
 * (see Viewport.tsx createEffect — set `axisLabels.visible` alongside
 * `axes.visible` so they toggle together with the Grid button).
 *
 * Returns `{ group, dispose, setOffset }`. Call `dispose()` in the owning component's
 * onCleanup to release the CanvasTexture and SpriteMaterial GPU resources
 * for each sprite (renderer.dispose() does NOT free per-object materials or
 * textures, so on Viewport unmount/remount these would otherwise leak).
 *
 * `setOffset(distance)` exists so scene.ts's `fitHelpers` can keep the labels just
 * beyond a scene-scaled axis tip. Callers must use it INSTEAD of scaling the Group,
 * for the shader reason in the COUPLING WARNING above.
 */
export function createAxisLabels(): {
  group: Group;
  dispose(): void;
  setOffset(distance: number): void;
} {
  const group = new Group();
  const sprites: Sprite[] = [];
  for (const spec of LABELS) {
    const sprite = makeTextSprite(spec);
    sprites.push(sprite);
    group.add(sprite);
  }

  /** Unit direction per axis, derived from the LABELS spec rather than re-deduced
   *  from the letter, so direction has exactly one definition in this module.
   *  LABEL_OFFSET divides out exactly (2.3/2.3 = 1, 0/2.3 = 0), so the units are
   *  exact and `setOffset(d)` lands on `d` and `0` with no float drift. */
  const unitByAxis = new Map<LabelSpec['axis'], [number, number, number]>(
    LABELS.map((spec) => [
      spec.axis,
      spec.position.map((c) => c / LABEL_OFFSET) as [number, number, number],
    ]),
  );

  /**
   * Move every label to `distance` along its own axis.
   *
   * Reposition — never scale the Group. The r183 sprite shader multiplies the
   * on-screen size by `length(modelMatrix[0].xyz)`, which includes ancestor scale,
   * so a Group scale would silently undo LABEL_SCREEN_SCALE's constant screen size.
   *
   * Non-finite or non-positive distances are ignored rather than applied: a
   * degenerate scene measurement upstream must leave a usable label ring standing,
   * not collapse it onto the origin, mirror it behind the origin, or poison the
   * sprite positions with NaN (which would remove them from the frustum entirely).
   */
  function setOffset(distance: number): void {
    if (!Number.isFinite(distance) || distance <= 0) return;
    for (const sprite of sprites) {
      const unit = unitByAxis.get(sprite.userData.axis as LabelSpec['axis']);
      if (!unit) continue;
      sprite.position.set(unit[0] * distance, unit[1] * distance, unit[2] * distance);
    }
  }

  function dispose(): void {
    for (const sprite of sprites) {
      sprite.material.map?.dispose();
      sprite.material.dispose();
    }
  }

  return { group, dispose, setOffset };
}
