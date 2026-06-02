/**
 * Shared Three.js mock classes for axis-label sprite tests.
 *
 * Used by both axisLabels.test.ts and scene.test.ts to avoid the two suites
 * drifting apart. Import via `await import('./threeAxisMocks')` inside an async
 * vi.mock factory, or require('./threeAxisMocks') in a sync factory.
 *
 * vi.fn() calls in class field initializers run per instance (not at class-
 * definition time), so each new sprite/material/texture gets its own spy.
 */
import { vi } from 'vitest';

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
  dispose = vi.fn();
}

export class MockSprite {
  type = 'Sprite';
  material: MockSpriteMaterial;
  name = '';
  userData: Record<string, any> = {};
  renderOrder = 0;
  scale = {
    x: 1, y: 1, z: 1,
    set: vi.fn((_x: number, _y: number, _z: number) => { /* recorded per instance */ }),
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
  constructor(canvas: any) {
    this.canvas = canvas;
  }
  dispose = vi.fn();
}

export class MockColor {
  value: any;
  constructor(v?: any) { this.value = v; }
}
