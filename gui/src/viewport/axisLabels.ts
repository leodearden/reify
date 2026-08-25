import {
  CanvasTexture,
  Group,
  Sprite,
  SpriteMaterial,
} from 'three';

import { AXIS_LABEL_RENDER_ORDER } from './renderOrder';

/**
 * Offset from origin where each axis label is placed.
 * Must be > AxesHelper size (2) so labels sit just beyond the axis tip.
 */
const LABEL_OFFSET = 2.3;

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
    // Depth-tested: labels are world-space geometry at the axis tips, so model
    // geometry between them and the camera must occlude them (#6587). They still
    // write no depth — see the helper rule in ./renderOrder.ts.
    depthTest: true,
    depthWrite: false,
    transparent: true,
  });

  const sprite = new Sprite(material);
  sprite.name = `axis-label-${spec.axis}`;
  sprite.userData.axis = spec.axis;
  sprite.renderOrder = AXIS_LABEL_RENDER_ORDER;
  sprite.scale.set(LABEL_SCALE, LABEL_SCALE, 1);
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
