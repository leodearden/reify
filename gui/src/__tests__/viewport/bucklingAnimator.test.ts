/**
 * Tests for createBucklingAnimator().
 *
 * Task ι/3458. Verifies the connectivity-free point-cloud animator:
 *   - builds a BufferGeometry with a Float32 'position' attribute
 *   - update(positions) writes values in-place and sets needsUpdate=true
 *   - setUndeformedVisible(bool) toggles the overlay object's .visible
 *   - dispose() cleans up geometry and material
 *
 * Uses the `three` vi.mock pattern from meshManager.test.ts with
 * MockBufferGeometry / MockBufferAttribute from threeMocks.ts.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Three.js mock ────────────────────────────────────────────────────────────

const mockGeometries: any[] = [];
const mockMaterials: any[] = [];

vi.mock('three', async () => {
  class MockBufferGeometry {
    attributes: Record<string, any> = {};
    dispose = vi.fn();

    setAttribute(name: string, attr: any) {
      this.attributes[name] = attr;
    }

    getAttribute(name: string): any {
      return this.attributes[name];
    }

    constructor() {
      mockGeometries.push(this);
    }
  }

  class MockBufferAttribute {
    array: Float32Array;
    itemSize: number;
    needsUpdate = false;
    constructor(array: Float32Array, itemSize: number) {
      this.array = array;
      this.itemSize = itemSize;
    }
  }

  class MockPoints {
    geometry: any;
    material: any;
    visible = true;
    constructor(geometry: any, material: any) {
      this.geometry = geometry;
      this.material = material;
    }
  }

  class MockPointsMaterial {
    color: any;
    size: number;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = opts?.color;
      this.size = opts?.size ?? 1;
      mockMaterials.push(this);
    }
  }

  class MockColor {
    constructor(_v?: any) {}
  }

  return {
    BufferGeometry: MockBufferGeometry,
    BufferAttribute: MockBufferAttribute,
    Float32BufferAttribute: MockBufferAttribute,
    Points: MockPoints,
    PointsMaterial: MockPointsMaterial,
    Color: MockColor,
  };
});

// ── Subject under test ───────────────────────────────────────────────────────

import { createBucklingAnimator } from '../../viewport/bucklingAnimator';

// ── Setup ────────────────────────────────────────────────────────────────────

beforeEach(() => {
  mockGeometries.length = 0;
  mockMaterials.length = 0;
  vi.clearAllMocks();
});

// ── Tests ────────────────────────────────────────────────────────────────────

const BASE = [0, 0, 0, 1, 0, 0, 0, 1, 0]; // 3 nodes × 3 floats

describe('createBucklingAnimator', () => {
  it('creates a BufferGeometry with a "position" attribute sized to base positions', () => {
    const animator = createBucklingAnimator(BASE);
    // One geometry should have been created for the displaced point cloud
    const geom = mockGeometries.find(g => g.attributes['position']);
    expect(geom).toBeDefined();
    const posAttr = geom.attributes['position'];
    expect(posAttr).toBeDefined();
    expect(posAttr.array.length).toBe(BASE.length);
    animator.dispose();
  });

  it('exposes an object3d', () => {
    const animator = createBucklingAnimator(BASE);
    expect(animator.object3d).toBeDefined();
    animator.dispose();
  });

  it('update(positions) writes values into posAttr.array in place', () => {
    const animator = createBucklingAnimator(BASE);
    const displacedGeom = mockGeometries.find(g => g.attributes['position']);
    const posAttr = displacedGeom.attributes['position'];

    const newPositions = [0.1, 0.2, 0.3, 1.1, 0.2, 0.3, 0.1, 1.2, 0.3];
    animator.update(newPositions);

    for (let i = 0; i < newPositions.length; i++) {
      expect(posAttr.array[i]).toBeCloseTo(newPositions[i]!, 6);
    }
    animator.dispose();
  });

  it('update(positions) sets posAttr.needsUpdate = true', () => {
    const animator = createBucklingAnimator(BASE);
    const displacedGeom = mockGeometries.find(g => g.attributes['position']);
    const posAttr = displacedGeom.attributes['position'];

    posAttr.needsUpdate = false;
    animator.update(BASE);
    expect(posAttr.needsUpdate).toBe(true);
    animator.dispose();
  });

  it('setUndeformedVisible(true) makes the overlay visible', () => {
    const animator = createBucklingAnimator(BASE);
    animator.setUndeformedVisible(true);
    // The overlay object's visible property should be true
    expect(animator.undeformedOverlay.visible).toBe(true);
    animator.dispose();
  });

  it('setUndeformedVisible(false) hides the overlay', () => {
    const animator = createBucklingAnimator(BASE);
    animator.setUndeformedVisible(true);
    animator.setUndeformedVisible(false);
    expect(animator.undeformedOverlay.visible).toBe(false);
    animator.dispose();
  });

  it('dispose() calls geometry dispose', () => {
    const animator = createBucklingAnimator(BASE);
    animator.dispose();
    // All created geometries should have had dispose() called
    for (const g of mockGeometries) {
      expect(g.dispose).toHaveBeenCalled();
    }
  });

  it('dispose() calls material dispose', () => {
    const animator = createBucklingAnimator(BASE);
    animator.dispose();
    for (const m of mockMaterials) {
      expect(m.dispose).toHaveBeenCalled();
    }
  });
});
