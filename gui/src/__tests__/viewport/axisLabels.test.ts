// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';

// Self-contained vi.mock('three') for axisLabels unit tests.
// Captures ctor args so tests can assert color/position/flags without a real WebGL context.

const mockGroupAdd = vi.fn();

vi.mock('three', () => {
  class MockGroup {
    type = 'Group';
    children: any[] = [];
    visible = true;
    renderOrder = 0;
    add(obj: any) {
      this.children.push(obj);
      mockGroupAdd(obj);
    }
  }

  class MockSpriteMaterial {
    map: any;
    color: any;
    depthTest: boolean;
    depthWrite: boolean;
    transparent: boolean;
    constructor(opts: any = {}) {
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
    scale = { x: 1, y: 1, z: 1, set: vi.fn((x: number, y: number, z: number) => { /* recorded */ }) };
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
  mockGroupAdd.mockClear();
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

  it('labels are identified as X, Y, Z via sprite.name', () => {
    const group = createAxisLabels();
    const names = group.children.map((s: any) => {
      // Accept either 'axis-label-X' or just 'X', or userData.axis
      if (s.name && s.name.includes('X')) return 'X';
      if (s.name && s.name.includes('Y')) return 'Y';
      if (s.name && s.name.includes('Z')) return 'Z';
      if (s.userData?.axis) return s.userData.axis;
      return null;
    });
    expect(names).toContain('X');
    expect(names).toContain('Y');
    expect(names).toContain('Z');
  });

  it('X label color is red (0xff0000)', () => {
    const group = createAxisLabels();
    const xSprite = group.children.find(
      (s: any) => s.name?.includes('X') || s.userData?.axis === 'X',
    );
    expect(xSprite).toBeDefined();
    expect(xSprite.material.color).toBe(0xff0000);
  });

  it('Y label color is green (0x00ff00)', () => {
    const group = createAxisLabels();
    const ySprite = group.children.find(
      (s: any) => s.name?.includes('Y') || s.userData?.axis === 'Y',
    );
    expect(ySprite).toBeDefined();
    expect(ySprite.material.color).toBe(0x00ff00);
  });

  it('Z label color is blue (0x0000ff)', () => {
    const group = createAxisLabels();
    const zSprite = group.children.find(
      (s: any) => s.name?.includes('Z') || s.userData?.axis === 'Z',
    );
    expect(zSprite).toBeDefined();
    expect(zSprite.material.color).toBe(0x0000ff);
  });

  it('X label is positioned beyond the X axis tip (x > 2, y === 0, z === 0)', () => {
    const group = createAxisLabels();
    const xSprite = group.children.find(
      (s: any) => s.name?.includes('X') || s.userData?.axis === 'X',
    );
    expect(xSprite.position.x).toBeGreaterThan(2);
    expect(xSprite.position.y).toBe(0);
    expect(xSprite.position.z).toBe(0);
  });

  it('Y label is positioned beyond the Y axis tip (y > 2, x === 0, z === 0)', () => {
    const group = createAxisLabels();
    const ySprite = group.children.find(
      (s: any) => s.name?.includes('Y') || s.userData?.axis === 'Y',
    );
    expect(ySprite.position.y).toBeGreaterThan(2);
    expect(ySprite.position.x).toBe(0);
    expect(ySprite.position.z).toBe(0);
  });

  it('Z label is positioned beyond the Z axis tip (z > 2, x === 0, y === 0)', () => {
    const group = createAxisLabels();
    const zSprite = group.children.find(
      (s: any) => s.name?.includes('Z') || s.userData?.axis === 'Z',
    );
    expect(zSprite.position.z).toBeGreaterThan(2);
    expect(zSprite.position.x).toBe(0);
    expect(zSprite.position.y).toBe(0);
  });

  it('all sprites have depthTest === false (always-on-top)', () => {
    const group = createAxisLabels();
    for (const child of group.children) {
      expect(child.material.depthTest).toBe(false);
    }
  });

  it('all sprites have depthWrite === false (always-on-top)', () => {
    const group = createAxisLabels();
    for (const child of group.children) {
      expect(child.material.depthWrite).toBe(false);
    }
  });

  it('all sprites have renderOrder > 0', () => {
    const group = createAxisLabels();
    for (const child of group.children) {
      expect(child.renderOrder).toBeGreaterThan(0);
    }
  });

  it('all sprites have a non-degenerate positive scale', () => {
    const group = createAxisLabels();
    for (const child of group.children) {
      // scale is set via set() — check that scale.set was called with positive values
      expect(child.scale.set).toHaveBeenCalled();
      const setArgs = child.scale.set.mock.calls[0];
      expect(setArgs[0]).toBeGreaterThan(0);
      expect(setArgs[1]).toBeGreaterThan(0);
    }
  });
});
