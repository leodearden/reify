import {
  CanvasTexture,
  Group,
  Sprite,
  SpriteMaterial,
} from 'three';

/**
 * Offset from origin where each axis label is placed.
 * Must be > AxesHelper size (2) so labels sit just beyond the axis tip.
 */
const LABEL_OFFSET = 2.3;

/**
 * Render order for axis labels — above axes (renderOrder 1) so labels are
 * always visible when the axes overlay is visible.
 */
const LABEL_RENDER_ORDER = 2;

/**
 * Size (Three.js world units) of the label sprite quad.
 */
const LABEL_SCALE = 0.5;

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
  canvas.width = 64;
  canvas.height = 64;

  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.clearRect(0, 0, 64, 64);
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 48px sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(spec.axis, 32, 32);
  }

  const texture = new CanvasTexture(canvas);
  const material = new SpriteMaterial({
    map: texture,
    color: spec.color,
    depthTest: false,
    depthWrite: false,
    transparent: true,
  });

  const sprite = new Sprite(material);
  sprite.name = `axis-label-${spec.axis}`;
  sprite.userData.axis = spec.axis;
  sprite.renderOrder = LABEL_RENDER_ORDER;
  sprite.scale.set(LABEL_SCALE, LABEL_SCALE, 1);
  sprite.position.set(...spec.position);

  return sprite;
}

/**
 * Create a Group containing three camera-facing "X"/"Y"/"Z" text sprites
 * positioned just beyond the tips of the AxesHelper triad.
 *
 * The labels render always-on-top (depthTest=false, depthWrite=false,
 * renderOrder > axes renderOrder=1) so the coplanar grid never occludes them.
 *
 * Visibility should be driven by the same signal that controls the axes
 * (see Viewport.tsx createEffect — set `axisLabels.visible` alongside
 * `axes.visible` so they toggle together with the Grid button).
 *
 * Returns `{ group, dispose }`. Call `dispose()` in the owning component's
 * onCleanup to release the CanvasTexture and SpriteMaterial GPU resources
 * for each sprite (renderer.dispose() does NOT free per-object materials or
 * textures, so on Viewport unmount/remount these would otherwise leak).
 */
export function createAxisLabels(): { group: Group; dispose(): void } {
  const group = new Group();
  const sprites: Sprite[] = [];
  for (const spec of LABELS) {
    const sprite = makeTextSprite(spec);
    sprites.push(sprite);
    group.add(sprite);
  }

  function dispose(): void {
    for (const sprite of sprites) {
      sprite.material.map?.dispose();
      sprite.material.dispose();
    }
  }

  return { group, dispose };
}
