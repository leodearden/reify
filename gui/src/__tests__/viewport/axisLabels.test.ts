// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock classes shared with scene.test.ts — imported from threeAxisMocks.ts to
// keep both suites in sync and prevent silent drift between the two mocks.

vi.mock('three', async () => {
  const {
    MockGroup,
    MockSprite,
    MockSpriteMaterial,
    MockCanvasTexture,
    MockColor,
    LinearFilter,
  } = await import('./threeAxisMocks');
  return {
    Group: MockGroup,
    Sprite: MockSprite,
    SpriteMaterial: MockSpriteMaterial,
    CanvasTexture: MockCanvasTexture,
    Color: MockColor,
    // axisLabels.ts imports LinearFilter from 'three' to pin the label texture's
    // minification filter (#6588); the factory must supply it or the import is
    // undefined at module load.
    LinearFilter,
  };
});

import { createAxisLabels } from '../../viewport/axisLabels';
// The same sentinel object the vi.mock('three') factory above hands to
// axisLabels.ts, so `map.minFilter === LinearFilter` is a real identity check.
import { LinearFilter } from './threeAxisMocks';
import { AXES_RENDER_ORDER, AXIS_LABEL_RENDER_ORDER } from '../../viewport/renderOrder';

beforeEach(() => {
  vi.clearAllMocks();
});

describe('createAxisLabels', () => {
  it('returns { group, dispose }', () => {
    const result = createAxisLabels();
    expect(result).toHaveProperty('group');
    expect(typeof result.dispose).toBe('function');
  });

  it('group is a Group', () => {
    const { group } = createAxisLabels();
    expect(group.type).toBe('Group');
  });

  it('group has exactly 3 children', () => {
    const { group } = createAxisLabels();
    expect(group.children).toHaveLength(3);
  });

  it('all children are Sprites', () => {
    const { group } = createAxisLabels();
    for (const child of group.children) {
      expect(child.type).toBe('Sprite');
    }
  });

  it('labels identified by exact sprite.name AND sprite.userData.axis — both must be present', () => {
    const { group } = createAxisLabels();
    const xSprite = group.children.find((s: any) => s.name === 'axis-label-X') as any;
    const ySprite = group.children.find((s: any) => s.name === 'axis-label-Y') as any;
    const zSprite = group.children.find((s: any) => s.name === 'axis-label-Z') as any;

    expect(xSprite).toBeDefined();
    expect(xSprite.userData.axis).toBe('X');

    expect(ySprite).toBeDefined();
    expect(ySprite.userData.axis).toBe('Y');

    expect(zSprite).toBeDefined();
    expect(zSprite.userData.axis).toBe('Z');
  });

  it('X label color is red — value 0xff0000 passed to SpriteMaterial ctor', () => {
    const { group } = createAxisLabels();
    const xSprite = group.children.find((s: any) => s.name === 'axis-label-X') as any;
    expect(xSprite).toBeDefined();
    // Assert on ctorOpts.color (the value passed to the constructor) rather than
    // material.color, which would only reflect the mock's pass-through behavior.
    expect(xSprite.material.ctorOpts.color).toBe(0xff0000);
  });

  it('Y label color is green — value 0x00ff00 passed to SpriteMaterial ctor', () => {
    const { group } = createAxisLabels();
    const ySprite = group.children.find((s: any) => s.name === 'axis-label-Y') as any;
    expect(ySprite).toBeDefined();
    expect(ySprite.material.ctorOpts.color).toBe(0x00ff00);
  });

  it('Z label color is blue — value 0x0000ff passed to SpriteMaterial ctor', () => {
    const { group } = createAxisLabels();
    const zSprite = group.children.find((s: any) => s.name === 'axis-label-Z') as any;
    expect(zSprite).toBeDefined();
    expect(zSprite.material.ctorOpts.color).toBe(0x0000ff);
  });

  it('X label is positioned beyond the X axis tip (x > 2, y === 0, z === 0)', () => {
    const { group } = createAxisLabels();
    const xSprite = group.children.find((s: any) => s.name === 'axis-label-X') as any;
    expect(xSprite.position.x).toBeGreaterThan(2);
    expect(xSprite.position.y).toBe(0);
    expect(xSprite.position.z).toBe(0);
  });

  it('Y label is positioned beyond the Y axis tip (y > 2, x === 0, z === 0)', () => {
    const { group } = createAxisLabels();
    const ySprite = group.children.find((s: any) => s.name === 'axis-label-Y') as any;
    expect(ySprite.position.y).toBeGreaterThan(2);
    expect(ySprite.position.x).toBe(0);
    expect(ySprite.position.z).toBe(0);
  });

  it('Z label is positioned beyond the Z axis tip (z > 2, x === 0, y === 0)', () => {
    const { group } = createAxisLabels();
    const zSprite = group.children.find((s: any) => s.name === 'axis-label-Z') as any;
    expect(zSprite.position.z).toBeGreaterThan(2);
    expect(zSprite.position.x).toBe(0);
    expect(zSprite.position.y).toBe(0);
  });

  it('label sprites are depth-tested so geometry between the camera and the label tip occludes them (#6587)', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      // Assert on ctorOpts, NOT material.depthTest: MockSpriteMaterial defaults
      // depthTest to `opts.depthTest ?? true`, so a material.depthTest assertion
      // would pass vacuously if the option were dropped from axisLabels.ts entirely.
      expect(child.material.ctorOpts.depthTest).toBe(true);
    }
  });

  it('label sprites write no depth and stay transparent', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.material.ctorOpts.depthWrite).toBe(false);
      expect(child.material.ctorOpts.transparent).toBe(true);
    }
  });

  it('label sprites sit at the top of the helper tier, after the axes they annotate', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.renderOrder).toBe(AXIS_LABEL_RENDER_ORDER);
      expect(child.renderOrder).toBeGreaterThan(AXES_RENDER_ORDER);
    }
  });

  it('all sprites have a non-degenerate positive scale', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      // scale is set via set() — check that scale.set was called with positive values
      expect(child.scale.set).toHaveBeenCalled();
      const setArgs = (child.scale.set as any).mock.calls[0];
      expect(setArgs[0]).toBeGreaterThan(0);
      expect(setArgs[1]).toBeGreaterThan(0);
    }
  });

  it('dispose() calls material.dispose() and material.map.dispose() for each sprite', () => {
    const { group, dispose } = createAxisLabels();
    dispose();
    for (const child of group.children as any[]) {
      expect(child.material.dispose).toHaveBeenCalledTimes(1);
      expect(child.material.map.dispose).toHaveBeenCalledTimes(1);
    }
  });
});

// ── setOffset tests (#6588) ──────────────────────────────────────────────────
// The label ring must be able to follow a SCENE-SIZED axis triad (scene.ts's
// fitHelpers), and it must do so by repositioning the sprites — not by scaling
// the Group, which the r183 sprite shader would fold into the on-screen size via
// length(modelMatrix[0].xyz), undoing the constant-screen-size fix above.

describe('createAxisLabels setOffset (#6588)', () => {
  /** Sprite lookup by its own declared axis, not by array order. */
  function byAxis(group: any, axis: 'X' | 'Y' | 'Z'): any {
    const sprite = group.children.find((s: any) => s.userData.axis === axis);
    expect(sprite).toBeDefined();
    return sprite;
  }

  function expectRingAt(group: any, d: number): void {
    expect(byAxis(group, 'X').position.x).toBe(d);
    expect(byAxis(group, 'X').position.y).toBe(0);
    expect(byAxis(group, 'X').position.z).toBe(0);

    expect(byAxis(group, 'Y').position.y).toBe(d);
    expect(byAxis(group, 'Y').position.x).toBe(0);
    expect(byAxis(group, 'Y').position.z).toBe(0);

    expect(byAxis(group, 'Z').position.z).toBe(d);
    expect(byAxis(group, 'Z').position.x).toBe(0);
    expect(byAxis(group, 'Z').position.y).toBe(0);
  }

  it('exposes setOffset alongside group and dispose', () => {
    const result = createAxisLabels() as any;
    expect(typeof result.setOffset).toBe('function');
  });

  it('setOffset(d) moves each sprite to d along its own axis, zero on the other two', () => {
    const result = createAxisLabels() as any;
    result.setOffset(0.35);
    expectRingAt(result.group, 0.35);
  });

  it.each([
    ['zero', 0],
    ['negative', -1],
    ['NaN', NaN],
    ['Infinity', Infinity],
  ])('setOffset(%s) is a no-op that leaves the previous offset in place', (_label, bad) => {
    const result = createAxisLabels() as any;
    result.setOffset(0.35);
    result.setOffset(bad as number);
    // A degenerate offset must not collapse the ring onto the origin, fold it
    // behind the origin, or poison the positions with NaN/Infinity.
    expectRingAt(result.group, 0.35);
  });

  it('leaves the construction-time offset untouched until setOffset is called', () => {
    const result = createAxisLabels() as any;
    // Same contract the "positioned beyond the axis tip" tests above assert: the
    // default ring sits beyond the default AxesHelper(2) tip.
    expect(byAxis(result.group, 'X').position.x).toBeGreaterThan(2);
    expect(byAxis(result.group, 'Y').position.y).toBeGreaterThan(2);
    expect(byAxis(result.group, 'Z').position.z).toBeGreaterThan(2);
  });
});

// ── Screen-footprint tests (#6588) ───────────────────────────────────────────

describe('axis label screen footprint (#6588)', () => {
  // The app's PerspectiveCamera is constructed with fov = 60 (scene.ts).
  const FOV_DEG = 60;

  /**
   * On-screen height of a sprite as a FRACTION of the viewport height, under the
   * three r183 sprite vertex shader (sprite.glsl.js), for a material with
   * `sizeAttenuation: false`:
   *
   *     vec4 mvPosition = modelViewMatrix[3];
   *     vec2 scale = vec2(length(modelMatrix[0].xyz), length(modelMatrix[1].xyz));
   *     #ifndef USE_SIZEATTENUATION
   *       if (isPerspective) scale *= -mvPosition.z;
   *     #endif
   *     vec2 alignedPosition = (position.xy - (center - vec2(0.5))) * scale;
   *
   * `position.xy` spans [-0.5, 0.5], and that `scale *= -mvPosition.z` CANCELS the
   * perspective divide that follows. So the fraction reduces to
   *
   *     f = s * cot(fov/2) / 2
   *
   * with NO camera-distance term `d` — which is the whole point of the fix.
   */
  function screenHeightFraction(s: number): number {
    return (s * (1 / Math.tan((FOV_DEG * Math.PI) / 180 / 2))) / 2;
  }

  // REGRESSION REPRO (#6588, dogfood session). With three.js's DEFAULT
  // `sizeAttenuation: true`, the `-mvPosition.z` factor is absent, the perspective
  // divide survives, and the fraction becomes distance-dependent:
  //     f = worldScale * cot(fov/2) / (2 * d)
  // Reported camera (0.2923, -0.2809, 1.8260); the Z label sits at (0, 0, 2.3), so
  //     d = sqrt(0.2923^2 + 0.2809^2 + 0.4740^2) = 0.6237
  //     f = 0.5 * 1.7320508 / (2 * 0.6237) = 0.694
  // i.e. the "Z" glyph covered 69% of the frame height, sourced from a 64-texel
  // texture — ~9x bilinear magnification at DPR 1. That is the reported blocky,
  // stair-stepped cyan/azure band terminating in a wedge. `sizeAttenuation: false`
  // removes the `d` term entirely, so NO camera position can reproduce it.

  it('sprites use sizeAttenuation: false, making their screen size camera-independent', () => {
    const { group } = createAxisLabels();
    expect(group.children).toHaveLength(3);
    for (const child of group.children as any[]) {
      // Assert on ctorOpts (the value handed to the constructor), matching how the
      // colour/depth tests above assert. MockSpriteMaterial leaves the field
      // undefined when the option is absent, so this cannot pass vacuously.
      expect(child.material.ctorOpts.sizeAttenuation).toBe(false);
    }
  });

  it('sprite scale keeps each label under 10% of the viewport height at every camera distance', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.scale.set).toHaveBeenCalled();
      const [sx, sy] = (child.scale.set as any).mock.calls[0];
      expect(sx).toBeGreaterThan(0);
      // Square quad: the glyph texture is square, so a non-square scale would
      // stretch the letter.
      expect(sy).toBe(sx);

      const frac = screenHeightFraction(sx);
      expect(frac).toBeGreaterThan(0);
      expect(frac).toBeLessThanOrEqual(0.1);
    }
  });

  it('leaves the sprite quad\'s unused third scale component at 1', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      const [, , sz] = (child.scale.set as any).mock.calls[0];
      expect(sz).toBe(1);
      // The mock writes back, so the resulting scale.z is observable too.
      expect(child.scale.z).toBe(1);
    }
  });
});

// ── Glyph drawing tests ──────────────────────────────────────────────────────
// jsdom returns null for getContext('2d') by default, which causes makeTextSprite
// to skip the drawing path. These tests stub getContext to verify that fillText
// IS called with each axis letter when a real 2D context is available.
//
// Follows the vi.spyOn(HTMLCanvasElement.prototype, 'getContext') precedent from
// BucklingPanel.test.tsx. Scoped to this describe so the structural tests above
// still exercise the null-context guard path without interference.

describe('createAxisLabels — glyph drawing (getContext truthy)', () => {
  let mockFillText: ReturnType<typeof vi.fn>;
  /** ctx.font as it stood at each fillText call — pins the font that was actually
   *  IN FORCE when the glyph was drawn, not merely the last one ever assigned. */
  let fontsAtDraw: string[];

  beforeEach(() => {
    fontsAtDraw = [];
    const mockCtx = {
      clearRect: vi.fn(),
      fillText: vi.fn(() => {
        fontsAtDraw.push(mockCtx.font);
      }),
      fillStyle: '',
      font: '',
      textAlign: '',
      textBaseline: '',
    };
    mockFillText = mockCtx.fillText;
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
      (contextId: string) => (contextId === '2d' ? (mockCtx as any) : null),
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('calls fillText with each axis letter when getContext returns a truthy 2D context', () => {
    createAxisLabels();
    const lettersDrawn = mockFillText.mock.calls.map((c: any[]) => c[0]);
    expect(lettersDrawn).toContain('X');
    expect(lettersDrawn).toContain('Y');
    expect(lettersDrawn).toContain('Z');
  });

  // ── Texel budget and filtering (#6588) ─────────────────────────────────────
  // Halving the label's screen size (step-2) is only half the cure for the
  // reported staircase: the glyph also needs enough texels to survive, and must
  // not be resolved through a blurred mip once it is usually MINIFIED.

  it('draws each glyph into a square canvas of at least 128 px so it has texels to spare', () => {
    const { group } = createAxisLabels();
    expect(group.children).toHaveLength(3);
    for (const child of group.children as any[]) {
      const canvas = child.material.map.canvas;
      expect(canvas.width).toBe(canvas.height);
      // 4.8% of a 1600-device-px-tall HiDPI viewport is ~77 px, so 128 leaves
      // headroom and the glyph is never magnified. 64 (the #6588 value) does not.
      expect(canvas.width).toBeGreaterThanOrEqual(128);
    }
  });

  it('scales the font with the canvas so more texels mean a bigger letter, not a smaller one', () => {
    const { group } = createAxisLabels();
    const edge = (group.children[0] as any).material.map.canvas.width;
    expect(fontsAtDraw).toHaveLength(3);
    for (const font of fontsAtDraw) {
      const px = /(\d+(?:\.\d+)?)px/.exec(font);
      expect(px).not.toBeNull();
      // A fixed 48px font in a 128px canvas would shrink the letter to 37% of the
      // texture and waste the extra resolution on empty margin.
      expect(Number(px![1])).toBeGreaterThanOrEqual(0.6 * edge);
    }
  });

  it('pins the label texture to linear minification with no mipmaps', () => {
    const { group } = createAxisLabels();
    for (const child of group.children as any[]) {
      // CanvasTexture inherits minFilter = LinearMipmapLinearFilter. Now that the
      // label is a fixed ~4.8% of the frame it is usually MINIFIED, so that default
      // would sample a blurred mip of a 128-px letter instead of the letter.
      // MockCanvasTexture starts both fields undefined, so this cannot pass vacuously.
      expect(child.material.map.minFilter).toBe(LinearFilter);
      expect(child.material.map.generateMipmaps).toBe(false);
    }
  });
});
