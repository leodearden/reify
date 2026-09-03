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

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

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

import { createBucklingAnimator, computePointCloudBounds } from '../../viewport/bucklingAnimator';

// ── Setup ────────────────────────────────────────────────────────────────────

beforeEach(() => {
  mockGeometries.length = 0;
  mockMaterials.length = 0;
  vi.clearAllMocks();
});

// ── Tests ────────────────────────────────────────────────────────────────────

const BASE = [0, 0, 0, 1, 0, 0, 0, 1, 0]; // 3 nodes × 3 floats

describe('computePointCloudBounds', () => {
  it('returns center and radius for a non-empty flat XYZ array', () => {
    // 3 nodes: (0,0,0), (2,0,0), (0,2,0)
    const positions = [0, 0, 0, 2, 0, 0, 0, 2, 0];
    const { center, radius } = computePointCloudBounds(positions);
    // bbox: x[0,2] y[0,2] z[0,0] → center [1,1,0]
    expect(center[0]).toBeCloseTo(1, 10);
    expect(center[1]).toBeCloseTo(1, 10);
    expect(center[2]).toBeCloseTo(0, 10);
    // half space-diagonal of bbox (2,2,0): 0.5 * sqrt(4+4+0) = sqrt(2)
    expect(radius).toBeCloseTo(Math.SQRT2, 10);
  });

  it('returns { center:[0,0,0], radius:0 } for an empty array', () => {
    const { center, radius } = computePointCloudBounds([]);
    expect(center).toEqual([0, 0, 0]);
    expect(radius).toBe(0);
  });
});

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

// ── update() length-mismatch contract (task #6813) ───────────────────────────

/**
 * `dispGeom`'s position attribute is a fixed-size `Float32Array` allocated once
 * from `base` in the factory — WebGL buffers cannot be resized, which is the
 * whole point of task #6757.  So `update()` cannot grow to fit an over-long
 * `positions`, and must bound its copy loop by the DESTINATION length.
 *
 * Today the copy loop in `update()` runs to `positions.length`.  The
 * over-long case is safe only by accident: TypedArray out-of-bounds writes are
 * silent no-ops, a property of the array type rather than of the code.  Both
 * mismatch directions currently pass silently — the over-long case drops the
 * excess, the short case leaves a stale tail — with no diagnostic either way.
 *
 * The mocked `three` is deliberately retained here (unlike the real-three
 * meshManager files): `MockBufferAttribute.array` is a genuine `Float32Array`,
 * which is all this defect depends on.
 */
describe('createBucklingAnimator update() length mismatch (#6813)', () => {
  /** 2 nodes × 3 floats — a 6-slot fixed-size destination buffer. */
  const BASE_2 = [0, 0, 0, 1, 0, 0];

  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    warnSpy.mockRestore();
  });

  /** The displaced point cloud is the first geometry the factory creates. */
  function positionArray(): Float32Array {
    const geom = mockGeometries.find((g) => g.attributes['position']);
    expect(geom).toBeDefined();
    return geom.attributes['position'].array as Float32Array;
  }

  /** The single warn call's message, asserted to be exactly one call. */
  function soleWarning(): string {
    expect(warnSpy).toHaveBeenCalledTimes(1);
    return String(warnSpy.mock.calls[0]!.join(' '));
  }

  it('CASE A: warns naming both lengths when positions is longer than the buffer', () => {
    const animator = createBucklingAnimator(BASE_2);
    const arr = positionArray();
    expect(arr.length).toBe(6);

    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]);

    const msg = soleWarning();
    expect(msg).toMatch(/\b9\b/);
    expect(msg).toMatch(/\b6\b/);
    // The destination was neither resized nor left short: it holds exactly the
    // first 6 source values, which is also today's observable behaviour.
    expect(arr.length).toBe(6);
    expect(Array.from(arr)).toEqual([1, 2, 3, 4, 5, 6]);

    animator.dispose();
  });

  it('CASE B: warns naming both lengths when positions is shorter than the buffer', () => {
    const animator = createBucklingAnimator(BASE_2);
    const arr = positionArray();

    animator.update([7, 8, 9]);

    const msg = soleWarning();
    expect(msg).toMatch(/\b3\b/);
    expect(msg).toMatch(/\b6\b/);
    // The prefix is written; the tail is stale. Unchanged behaviour — the point
    // of the warning is that this is now observable rather than silent.
    expect(arr.length).toBe(6);
    expect(Array.from(arr).slice(0, 3)).toEqual([7, 8, 9]);

    animator.dispose();
  });

  it('CASE C: does not warn when positions exactly matches the buffer', () => {
    // Non-vacuity / no-false-positive guard: without this, an implementation
    // that warned unconditionally would pass CASE A and CASE B.
    const animator = createBucklingAnimator(BASE_2);
    const arr = positionArray();

    animator.update([1, 2, 3, 4, 5, 6]);

    expect(warnSpy).not.toHaveBeenCalled();
    expect(Array.from(arr)).toEqual([1, 2, 3, 4, 5, 6]);

    animator.dispose();
  });

  // ── Latching (#6813 amendment) ────────────────────────────────────────────
  //
  // `update()` is driven from a requestAnimationFrame loop (BucklingPanel), and
  // a mismatch is not necessarily transient: the animator is built once in
  // onMount from `store.state.base` and its Float32Array is fixed thereafter,
  // while `bucklingStore.ingestFrame` can replace `state.base` with a
  // different-length array after a re-solve/re-tessellation.  An unlatched
  // warn would then emit ~60×/s indefinitely, flooding devtools, taxing the
  // render loop with per-frame string concatenation, and burying the one
  // diagnostic it exists to surface.  (The `validateMeshData` precedent this
  // message shape follows is called once per sync, not once per frame, so the
  // convention does not transfer unchanged.)
  //
  // The latch is keyed on the offending length, not a bare boolean, so a
  // *different* mismatch is still reported — and it resets on a matching call
  // so a recurrence after recovery is reported too.

  it('CASE D: warns only once across repeated updates at the same mismatching length', () => {
    const animator = createBucklingAnimator(BASE_2);

    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    animator.update([9, 8, 7, 6, 5, 4, 3, 2, 1]);

    // One warning, not one per frame.
    const msg = soleWarning();
    expect(msg).toMatch(/\b9\b/);
    expect(msg).toMatch(/\b6\b/);
    // Clamping still applies on every call, latched or not.
    expect(Array.from(positionArray())).toEqual([9, 8, 7, 6, 5, 4]);

    animator.dispose();
  });

  it('CASE E: warns again when the mismatching length changes', () => {
    const animator = createBucklingAnimator(BASE_2);

    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]); // 9 vs 6 → warn
    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]); // latched → silent
    animator.update([1, 2, 3]); // 3 vs 6 → new shape, warn

    expect(warnSpy).toHaveBeenCalledTimes(2);
    expect(String(warnSpy.mock.calls[0]!.join(' '))).toMatch(/\b9\b/);
    expect(String(warnSpy.mock.calls[1]!.join(' '))).toMatch(/\b3\b/);

    animator.dispose();
  });

  it('CASE F: an intervening matching update re-arms the latch', () => {
    const animator = createBucklingAnimator(BASE_2);

    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]); // mismatch → warn
    animator.update([1, 2, 3, 4, 5, 6]); // matches → silent, re-arms
    animator.update([1, 2, 3, 4, 5, 6, 7, 8, 9]); // mismatch again → warn

    expect(warnSpy).toHaveBeenCalledTimes(2);

    animator.dispose();
  });

  it('CASE G: each animator latches independently', () => {
    // The latch must be per-animator closure state, not module state — two
    // viewports (or a re-mounted panel) must each get their own diagnostic.
    const first = createBucklingAnimator(BASE_2);
    first.update([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(warnSpy).toHaveBeenCalledTimes(1);

    const second = createBucklingAnimator(BASE_2);
    second.update([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(warnSpy).toHaveBeenCalledTimes(2);

    first.dispose();
    second.dispose();
  });
});
