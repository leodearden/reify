// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Self-contained vi.mock('three') for axisLabels unit tests.
// Captures ctor args so tests can assert color/position/flags without a real WebGL context.

vi.mock('three', () => {
  class MockGroup {
    type = 'Group';
    children: any[] = [];
    visible = true;
    renderOrder = 0;
    add(obj: any) {
      this.children.push(obj);
    }
  }

  class MockSpriteMaterial {
    map: any;
    color: any;
    depthTest: boolean;
    depthWrite: boolean;
    transparent: boolean;
    /** Raw options passed to the constructor — use for assertions about what value
     *  was supplied; avoids coupling tests to the mock's pass-through behavior. */
    ctorOpts: any;
    constructor(opts: any = {}) {
      this.ctorOpts = { ...opts };
      this.map = opts.map;
      this.color = opts.color;
      this.depthTest = opts.depthTest ?? true;
      this.depthWrite = opts.depthWrite ?? true;
      this.transparent = opts.transparent ?? false;
    }
  }

  class MockSprite {
    type = 'Sprite';
    material: MockSpriteMaterial;
    name = '';
    userData: Record<string, any> = {};
    renderOrder = 0;
    scale = { x: 1, y: 1, z: 1, set: vi.fn((_x: number, _y: number, _z: number) => { /* recorded per instance */ }) };
    position = {
      x: 0, y: 0, z: 0,
      set: vi.fn(function(this: any, x: number, y: number, z: number) {
        this.x = x; this.y = y; this.z = z;
      }),
    };
    constructor(mat: MockSpriteMaterial) {
      this.material = mat;
    }
  }

  class MockCanvasTexture {
    canvas: any;
    constructor(canvas: any) {
      this.canvas = canvas;
    }
  }

  class MockColor {
    value: any;
    constructor(v?: any) { this.value = v; }
  }

  return {
    Group: MockGroup,
    Sprite: MockSprite,
    SpriteMaterial: MockSpriteMaterial,
    CanvasTexture: MockCanvasTexture,
    Color: MockColor,
  };
});

import { createAxisLabels } from '../../viewport/axisLabels';

beforeEach(() => {
  vi.clearAllMocks();
});

describe('createAxisLabels', () => {
  it('returns a Group', () => {
    const group = createAxisLabels();
    expect(group.type).toBe('Group');
  });

  it('group has exactly 3 children', () => {
    const group = createAxisLabels();
    expect(group.children).toHaveLength(3);
  });

  it('all children are Sprites', () => {
    const group = createAxisLabels();
    for (const child of group.children) {
      expect(child.type).toBe('Sprite');
    }
  });

  it('labels identified by exact sprite.name AND sprite.userData.axis — both must be present', () => {
    const group = createAxisLabels();
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
    const group = createAxisLabels();
    const xSprite = group.children.find((s: any) => s.name === 'axis-label-X') as any;
    expect(xSprite).toBeDefined();
    // Assert on ctorOpts.color (the value passed to the constructor) rather than
    // material.color, which would only reflect the mock's pass-through behavior.
    expect(xSprite.material.ctorOpts.color).toBe(0xff0000);
  });

  it('Y label color is green — value 0x00ff00 passed to SpriteMaterial ctor', () => {
    const group = createAxisLabels();
    const ySprite = group.children.find((s: any) => s.name === 'axis-label-Y') as any;
    expect(ySprite).toBeDefined();
    expect(ySprite.material.ctorOpts.color).toBe(0x00ff00);
  });

  it('Z label color is blue — value 0x0000ff passed to SpriteMaterial ctor', () => {
    const group = createAxisLabels();
    const zSprite = group.children.find((s: any) => s.name === 'axis-label-Z') as any;
    expect(zSprite).toBeDefined();
    expect(zSprite.material.ctorOpts.color).toBe(0x0000ff);
  });

  it('X label is positioned beyond the X axis tip (x > 2, y === 0, z === 0)', () => {
    const group = createAxisLabels();
    const xSprite = group.children.find((s: any) => s.name === 'axis-label-X') as any;
    expect(xSprite.position.x).toBeGreaterThan(2);
    expect(xSprite.position.y).toBe(0);
    expect(xSprite.position.z).toBe(0);
  });

  it('Y label is positioned beyond the Y axis tip (y > 2, x === 0, z === 0)', () => {
    const group = createAxisLabels();
    const ySprite = group.children.find((s: any) => s.name === 'axis-label-Y') as any;
    expect(ySprite.position.y).toBeGreaterThan(2);
    expect(ySprite.position.x).toBe(0);
    expect(ySprite.position.z).toBe(0);
  });

  it('Z label is positioned beyond the Z axis tip (z > 2, x === 0, y === 0)', () => {
    const group = createAxisLabels();
    const zSprite = group.children.find((s: any) => s.name === 'axis-label-Z') as any;
    expect(zSprite.position.z).toBeGreaterThan(2);
    expect(zSprite.position.x).toBe(0);
    expect(zSprite.position.y).toBe(0);
  });

  it('all sprites have depthTest === false (always-on-top)', () => {
    const group = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.material.depthTest).toBe(false);
    }
  });

  it('all sprites have depthWrite === false (always-on-top)', () => {
    const group = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.material.depthWrite).toBe(false);
    }
  });

  it('all sprites have renderOrder > 0', () => {
    const group = createAxisLabels();
    for (const child of group.children as any[]) {
      expect(child.renderOrder).toBeGreaterThan(0);
    }
  });

  it('all sprites have a non-degenerate positive scale', () => {
    const group = createAxisLabels();
    for (const child of group.children as any[]) {
      // scale is set via set() — check that scale.set was called with positive values
      expect(child.scale.set).toHaveBeenCalled();
      const setArgs = (child.scale.set as any).mock.calls[0];
      expect(setArgs[0]).toBeGreaterThan(0);
      expect(setArgs[1]).toBeGreaterThan(0);
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

  beforeEach(() => {
    mockFillText = vi.fn();
    const mockCtx = {
      clearRect: vi.fn(),
      fillText: mockFillText,
      fillStyle: '',
      font: '',
      textAlign: '',
      textBaseline: '',
    };
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
});
