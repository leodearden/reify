/**
 * Shared Three.js mock classes for axis-label sprite tests.
 *
 * Used by both axisLabels.test.ts and scene.test.ts to avoid the two suites
 * drifting apart. Import via `await import('./threeAxisMocks')` inside an async
 * vi.mock factory, or require('./threeAxisMocks') in a sync factory.
 *
 * vi.fn() calls in class field initializers run per instance (not at class-
 * definition time), so each new sprite/material/texture gets its own spy.
 *
 * HOUSE RULE (same intent as the comment at scene.test.ts's helper-material mock):
 * every mock field starts UNDEFINED, never at three.js's real default. An assertion
 * must be able to distinguish "our production code wrote this" from "the three.js
 * default happened to agree" — seeding a real default makes the test vacuous.
 */
import { vi } from 'vitest';

/**
 * Stand-in for three's `LinearFilter` texture-filter constant.
 *
 * Identity-comparable sentinel: tests assert `texture.minFilter === LinearFilter`,
 * which only holds if axisLabels.ts assigned the value it imported from 'three'
 * (which the vi.mock factory maps to this object). A bare number could collide
 * with an unrelated default; an object literal cannot.
 */
export const LinearFilter = { __mock: 'LinearFilter' } as const;

export class MockGroup {
  type = 'Group';
  children: any[] = [];
  visible = true;
  renderOrder = 0;
  add(obj: any) {
    this.children.push(obj);
  }
}

export class MockSpriteMaterial {
  map: any;
  color: any;
  depthTest: boolean;
  depthWrite: boolean;
  transparent: boolean;
  /** Starts undefined when the option is absent — see the HOUSE RULE above: a
   *  `toBe(false)` assertion must fail if axisLabels.ts stops passing the option. */
  sizeAttenuation: boolean | undefined;
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
    this.sizeAttenuation = opts.sizeAttenuation;
  }
  dispose = vi.fn();
}

export class MockSprite {
  type = 'Sprite';
  material: MockSpriteMaterial;
  name = '';
  userData: Record<string, any> = {};
  renderOrder = 0;
  /** `set` both records (spy) and WRITES BACK, mirroring `position.set`, so tests
   *  can assert either on the call args or on the resulting scale.x/y/z. */
  scale = {
    x: 1, y: 1, z: 1,
    set: vi.fn(function(this: any, x: number, y: number, z: number) {
      this.x = x; this.y = y; this.z = z;
    }),
  };
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

export class MockCanvasTexture {
  canvas: any;
  /** Texture-filtering fields start undefined (HOUSE RULE): real three.js defaults
   *  these to LinearMipmapLinearFilter / LinearFilter / true, so seeding them would
   *  make the #6588 mip-blur assertions pass without axisLabels.ts writing anything. */
  minFilter: any;
  magFilter: any;
  generateMipmaps: boolean | undefined;
  constructor(canvas: any) {
    this.canvas = canvas;
  }
  dispose = vi.fn();
}

export class MockColor {
  value: any;
  constructor(v?: any) { this.value = v; }
}
