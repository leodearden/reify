import { describe, it, expect, vi, beforeAll } from 'vitest';
import { viridisLut, magmaLut, rainbowLut, applyColormap, bakeColours, type Range } from '../../viewport/colormap';

// ---------------------------------------------------------------------------
// Minimal Three.js mocks — prevents the ~5 s jsdom initialization overhead
// when the barrel (viewport/index) is dynamically imported in the barrel-wiring
// tests below. meshManager.ts and selection.ts both run module-level prototype
// patches that require real class objects (not undefined), so we provide stubs.
// ---------------------------------------------------------------------------
vi.mock('three', () => {
  class MockClass {}
  return {
    BufferGeometry: class MockBufferGeometry {},
    Mesh:           class MockMesh {},
    Scene:              MockClass,
    PerspectiveCamera:  MockClass,
    WebGLRenderer:      MockClass,
    AmbientLight:       MockClass,
    DirectionalLight:   MockClass,
    GridHelper:         MockClass,
    AxesHelper:         MockClass,
    Color:              MockClass,
    Vector3:            MockClass,
    Vector2:            MockClass,
    Box3:               MockClass,
    Raycaster:          MockClass,
    EdgesGeometry:      MockClass,
    LineSegments:       MockClass,
    LineBasicMaterial:  MockClass,
    BufferAttribute:    MockClass,
    MeshStandardMaterial: MockClass,
    MeshBasicMaterial:  MockClass,
    Group:              MockClass,
    DoubleSide: 2,
    FrontSide:  0,
  };
});

vi.mock('three-mesh-bvh', () => ({
  computeBoundsTree:  () => {},
  disposeBoundsTree:  () => {},
  acceleratedRaycast: () => {},
}));

vi.mock('three/addons/controls/OrbitControls.js', () => ({
  OrbitControls: class MockOrbitControls {},
}));

// wireManager.ts (imported transitively via Viewport.tsx) pulls in three/addons
// fat-line classes. Without this mock the lottie_canvas.module.js inside three/addons
// tries to create a Canvas2D context at module-init time, which returns null in jsdom
// and throws "Cannot set properties of null (setting 'fillStyle')".
vi.mock('three/addons', () => {
  class MockClass {}
  return {
    LineSegments2:        MockClass,
    LineSegmentsGeometry: class MockLineSegmentsGeometry { setPositions() {} dispose() {} },
    LineMaterial:         class MockLineMaterial { resolution = { set() {} }; dispose() {} },
  };
});

// The barrel's transitive import chain reaches bridge.ts → @tauri-apps/api.
// In jsdom (no Tauri IPC), these modules hang on initialisation, exceeding
// the 5 000 ms test timeout. Mock them the same way every other test file does.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Step 1 — LUT shape & published spot-check values
// ---------------------------------------------------------------------------
describe('LUT shape and spot-check values', () => {
  it('viridisLut is a Float32Array of length 768', () => {
    expect(viridisLut).toBeInstanceOf(Float32Array);
    expect(viridisLut.length).toBe(768);
  });

  it('magmaLut is a Float32Array of length 768', () => {
    expect(magmaLut).toBeInstanceOf(Float32Array);
    expect(magmaLut.length).toBe(768);
  });

  it('rainbowLut is a Float32Array of length 768', () => {
    expect(rainbowLut).toBeInstanceOf(Float32Array);
    expect(rainbowLut.length).toBe(768);
  });

  it('all viridisLut values are in [0, 1]', () => {
    for (let i = 0; i < viridisLut.length; i++) {
      expect(viridisLut[i]).toBeGreaterThanOrEqual(0);
      expect(viridisLut[i]).toBeLessThanOrEqual(1);
    }
  });

  it('all magmaLut values are in [0, 1]', () => {
    for (let i = 0; i < magmaLut.length; i++) {
      expect(magmaLut[i]).toBeGreaterThanOrEqual(0);
      expect(magmaLut[i]).toBeLessThanOrEqual(1);
    }
  });

  it('all rainbowLut values are in [0, 1]', () => {
    for (let i = 0; i < rainbowLut.length; i++) {
      expect(rainbowLut[i]).toBeGreaterThanOrEqual(0);
      expect(rainbowLut[i]).toBeLessThanOrEqual(1);
    }
  });

  it('viridisLut entry 0 matches matplotlib reference (0.267004, 0.004874, 0.329415) within 1e-5', () => {
    expect(viridisLut[0]).toBeCloseTo(0.267004, 5);
    expect(viridisLut[1]).toBeCloseTo(0.004874, 5);
    expect(viridisLut[2]).toBeCloseTo(0.329415, 5);
  });

  it('viridisLut entry 255 matches matplotlib reference (0.993248, 0.906157, 0.143936) within 1e-5', () => {
    expect(viridisLut[255 * 3 + 0]).toBeCloseTo(0.993248, 5);
    expect(viridisLut[255 * 3 + 1]).toBeCloseTo(0.906157, 5);
    expect(viridisLut[255 * 3 + 2]).toBeCloseTo(0.143936, 5);
  });

  it('magmaLut entry 0 matches matplotlib reference (0.001462, 0.000466, 0.013866) within 1e-5', () => {
    expect(magmaLut[0]).toBeCloseTo(0.001462, 5);
    expect(magmaLut[1]).toBeCloseTo(0.000466, 5);
    expect(magmaLut[2]).toBeCloseTo(0.013866, 5);
  });

  it('rainbowLut entry 0 starts blue (R≈0, B≈1)', () => {
    expect(rainbowLut[0]).toBeCloseTo(0, 1);     // R ≈ 0
    expect(rainbowLut[2]).toBeCloseTo(1, 1);     // B ≈ 1
  });

  it('rainbowLut entry 255 ends red (R≈1, B≈0)', () => {
    expect(rainbowLut[255 * 3 + 0]).toBeCloseTo(1, 1);  // R ≈ 1
    expect(rainbowLut[255 * 3 + 2]).toBeCloseTo(0, 1);  // B ≈ 0
  });
});

// ---------------------------------------------------------------------------
// Step 3 — applyColormap happy path & Range mode invariance
// ---------------------------------------------------------------------------
describe('applyColormap — happy path', () => {
  const fixedRange = { mode: 'fixed' as const, min: 0, max: 1 };

  it('value 0 maps to viridisLut entry 0', () => {
    const [r, g, b] = applyColormap(0, fixedRange, 'viridis');
    expect(r).toBeCloseTo(viridisLut[0], 5);
    expect(g).toBeCloseTo(viridisLut[1], 5);
    expect(b).toBeCloseTo(viridisLut[2], 5);
  });

  it('value 1 maps to viridisLut entry 255', () => {
    const [r, g, b] = applyColormap(1, fixedRange, 'viridis');
    expect(r).toBeCloseTo(viridisLut[255 * 3 + 0], 5);
    expect(g).toBeCloseTo(viridisLut[255 * 3 + 1], 5);
    expect(b).toBeCloseTo(viridisLut[255 * 3 + 2], 5);
  });

  it('value 0.5 linearly interpolates between viridisLut[127] and viridisLut[128]', () => {
    const [r, g, b] = applyColormap(0.5, fixedRange, 'viridis');
    // t=0.5 → fractional index = 127.5 → lerp between entries 127 and 128
    const lo = 127, hi = 128, frac = 0.5;
    const expR = viridisLut[lo * 3]     + frac * (viridisLut[hi * 3]     - viridisLut[lo * 3]);
    const expG = viridisLut[lo * 3 + 1] + frac * (viridisLut[hi * 3 + 1] - viridisLut[lo * 3 + 1]);
    const expB = viridisLut[lo * 3 + 2] + frac * (viridisLut[hi * 3 + 2] - viridisLut[lo * 3 + 2]);
    expect(r).toBeCloseTo(expR, 5);
    expect(g).toBeCloseTo(expG, 5);
    expect(b).toBeCloseTo(expB, 5);
  });

  it('magma value 0 maps to magmaLut entry 0', () => {
    const [r, g, b] = applyColormap(0, fixedRange, 'magma');
    expect(r).toBeCloseTo(magmaLut[0], 5);
    expect(g).toBeCloseTo(magmaLut[1], 5);
    expect(b).toBeCloseTo(magmaLut[2], 5);
  });

  it('rainbow value 1 maps to rainbowLut entry 255 (red)', () => {
    const [r, , b] = applyColormap(1, fixedRange, 'rainbow');
    expect(r).toBeCloseTo(rainbowLut[255 * 3 + 0], 5);
    expect(b).toBeCloseTo(rainbowLut[255 * 3 + 2], 5);
  });

  it('magma value 0.5 returns interpolated magmaLut result (not viridis)', () => {
    // Confirms resolveLut dispatches to the correct LUT for magma.
    const [r, g, b] = applyColormap(0.5, fixedRange, 'magma');
    const lo = 127, hi = 128, frac = 0.5;
    const expR = magmaLut[lo * 3]     + frac * (magmaLut[hi * 3]     - magmaLut[lo * 3]);
    const expG = magmaLut[lo * 3 + 1] + frac * (magmaLut[hi * 3 + 1] - magmaLut[lo * 3 + 1]);
    const expB = magmaLut[lo * 3 + 2] + frac * (magmaLut[hi * 3 + 2] - magmaLut[lo * 3 + 2]);
    expect(r).toBeCloseTo(expR, 5);
    expect(g).toBeCloseTo(expG, 5);
    expect(b).toBeCloseTo(expB, 5);
  });

  it('rainbow value 0.5 returns interpolated rainbowLut result (not viridis)', () => {
    // Confirms resolveLut dispatches to the correct LUT for rainbow.
    const [r, g, b] = applyColormap(0.5, fixedRange, 'rainbow');
    const lo = 127, hi = 128, frac = 0.5;
    const expR = rainbowLut[lo * 3]     + frac * (rainbowLut[hi * 3]     - rainbowLut[lo * 3]);
    const expG = rainbowLut[lo * 3 + 1] + frac * (rainbowLut[hi * 3 + 1] - rainbowLut[lo * 3 + 1]);
    const expB = rainbowLut[lo * 3 + 2] + frac * (rainbowLut[hi * 3 + 2] - rainbowLut[lo * 3 + 2]);
    expect(r).toBeCloseTo(expR, 5);
    expect(g).toBeCloseTo(expG, 5);
    expect(b).toBeCloseTo(expB, 5);
  });

  it('all three Range modes produce identical output for the same min/max', () => {
    const value = 0.3;
    const min = -10, max = 10;
    const autoRange   = { mode: 'auto'   as const, min, max };
    const fixedRange2 = { mode: 'fixed'  as const, min, max };
    const lockedRange = { mode: 'locked' as const, min, max, source: 'result @ 14:23' };

    const autoResult   = applyColormap(value, autoRange,   'viridis');
    const fixedResult  = applyColormap(value, fixedRange2, 'viridis');
    const lockedResult = applyColormap(value, lockedRange, 'viridis');

    expect(autoResult).toEqual(fixedResult);
    expect(lockedResult).toEqual(fixedResult);
  });
});

// ---------------------------------------------------------------------------
// Step 5 — out-of-range and NaN handling
// ---------------------------------------------------------------------------
describe('applyColormap — out-of-range and NaN handling', () => {
  const range: Range = { mode: 'fixed', min: 0, max: 1 };

  // Default saturation colours
  it('value > max returns black [0, 0, 0] by default', () => {
    expect(applyColormap(2, range, 'viridis')).toEqual([0, 0, 0]);
  });

  it('value < min returns grey [0.5, 0.5, 0.5] by default', () => {
    expect(applyColormap(-1, range, 'viridis')).toEqual([0.5, 0.5, 0.5]);
  });

  it('NaN returns grey [0.5, 0.5, 0.5] by default', () => {
    expect(applyColormap(NaN, range, 'viridis')).toEqual([0.5, 0.5, 0.5]);
  });

  // Option overrides
  it('aboveColor option overrides above-max colour', () => {
    expect(applyColormap(2, range, 'viridis', { aboveColor: [1, 0, 1] })).toEqual([1, 0, 1]);
  });

  it('belowColor option overrides below-min colour', () => {
    expect(applyColormap(-1, range, 'viridis', { belowColor: [0, 1, 1] })).toEqual([0, 1, 1]);
  });

  it('nanColor option overrides NaN colour', () => {
    expect(applyColormap(NaN, range, 'viridis', { nanColor: [1, 1, 0] })).toEqual([1, 1, 0]);
  });

  // Boundary inclusivity — exact min/max are in-range
  it('value exactly equal to range.max maps to viridisLut[255] (in-range)', () => {
    const [r, g, b] = applyColormap(1, range, 'viridis');
    expect(r).toBeCloseTo(viridisLut[255 * 3 + 0], 5);
    expect(g).toBeCloseTo(viridisLut[255 * 3 + 1], 5);
    expect(b).toBeCloseTo(viridisLut[255 * 3 + 2], 5);
  });

  it('value exactly equal to range.min maps to viridisLut[0] (in-range)', () => {
    const [r, g, b] = applyColormap(0, range, 'viridis');
    expect(r).toBeCloseTo(viridisLut[0], 5);
    expect(g).toBeCloseTo(viridisLut[1], 5);
    expect(b).toBeCloseTo(viridisLut[2], 5);
  });
});

// ---------------------------------------------------------------------------
// Degenerate and non-finite range bounds
// ---------------------------------------------------------------------------
describe('applyColormap — degenerate and non-finite range bounds', () => {
  it('degenerate range (min === max) maps in-range value to lut[0] (t=0)', () => {
    // span === 0 → t clamped to 0 → lerp returns lut entry 0.
    const degRange: Range = { mode: 'fixed', min: 5, max: 5 };
    const [r, g, b] = applyColormap(5, degRange, 'viridis');
    expect(r).toBeCloseTo(viridisLut[0], 5);
    expect(g).toBeCloseTo(viridisLut[1], 5);
    expect(b).toBeCloseTo(viridisLut[2], 5);
  });

  it('non-finite range.max (Infinity) returns nanColor default', () => {
    const badRange: Range = { mode: 'fixed', min: 0, max: Infinity };
    expect(applyColormap(0.5, badRange, 'viridis')).toEqual([0.5, 0.5, 0.5]);
  });

  it('non-finite range.min (NaN) returns nanColor default', () => {
    const badRange: Range = { mode: 'fixed', min: NaN, max: 1 };
    expect(applyColormap(0.5, badRange, 'viridis')).toEqual([0.5, 0.5, 0.5]);
  });

  it('non-finite range.max with custom nanColor returns the custom colour', () => {
    const badRange: Range = { mode: 'fixed', min: 0, max: Infinity };
    expect(applyColormap(0.5, badRange, 'viridis', { nanColor: [1, 0, 1] })).toEqual([1, 0, 1]);
  });

  it('bakeColours with non-finite range fills entire output with nanColor', () => {
    const badRange: Range = { mode: 'fixed', min: 0, max: Infinity };
    const out = bakeColours(new Float32Array([0, 0.5, 1]), badRange, 'viridis', { nanColor: [1, 0, 0] });
    expect(out[0]).toBeCloseTo(1, 5); expect(out[1]).toBeCloseTo(0, 5); expect(out[2]).toBeCloseTo(0, 5);
    expect(out[3]).toBeCloseTo(1, 5); expect(out[4]).toBeCloseTo(0, 5); expect(out[5]).toBeCloseTo(0, 5);
    expect(out[6]).toBeCloseTo(1, 5); expect(out[7]).toBeCloseTo(0, 5); expect(out[8]).toBeCloseTo(0, 5);
  });
});

// ---------------------------------------------------------------------------
// Step 7 — bakeColours
// ---------------------------------------------------------------------------
describe('bakeColours', () => {
  const range: Range = { mode: 'fixed', min: 0, max: 1 };
  const scalars = new Float32Array([0, 0.25, 0.5, 0.75, 1]);

  it('returns Float32Array of length scalars.length * 3', () => {
    const out = bakeColours(scalars, range, 'viridis');
    expect(out).toBeInstanceOf(Float32Array);
    expect(out.length).toBe(scalars.length * 3);
  });

  it('output matches per-element applyColormap for each scalar', () => {
    const out = bakeColours(scalars, range, 'viridis');
    for (let i = 0; i < scalars.length; i++) {
      const [r, g, b] = applyColormap(scalars[i], range, 'viridis');
      expect(out[i * 3]).toBeCloseTo(r, 5);
      expect(out[i * 3 + 1]).toBeCloseTo(g, 5);
      expect(out[i * 3 + 2]).toBeCloseTo(b, 5);
    }
  });

  it('handles NaN and out-of-range values per element', () => {
    const mixedScalars = new Float32Array([NaN, -1, 2]);
    const mixedRange: Range = { mode: 'fixed', min: 0, max: 1 };
    const out = bakeColours(mixedScalars, mixedRange, 'viridis');

    // NaN → grey
    expect(out[0]).toBeCloseTo(0.5, 5);
    expect(out[1]).toBeCloseTo(0.5, 5);
    expect(out[2]).toBeCloseTo(0.5, 5);

    // below-min → grey
    expect(out[3]).toBeCloseTo(0.5, 5);
    expect(out[4]).toBeCloseTo(0.5, 5);
    expect(out[5]).toBeCloseTo(0.5, 5);

    // above-max → black
    expect(out[6]).toBeCloseTo(0, 5);
    expect(out[7]).toBeCloseTo(0, 5);
    expect(out[8]).toBeCloseTo(0, 5);
  });

  it('options.aboveColor / belowColor / nanColor propagate through bakeColours', () => {
    const mixedScalars = new Float32Array([NaN, -1, 2]);
    const opts = {
      nanColor:   [1, 1, 0] as const,
      belowColor: [0, 1, 1] as const,
      aboveColor: [1, 0, 1] as const,
    };
    const out = bakeColours(mixedScalars, range, 'viridis', opts);

    // NaN → [1, 1, 0]
    expect(out[0]).toBeCloseTo(1, 5);
    expect(out[1]).toBeCloseTo(1, 5);
    expect(out[2]).toBeCloseTo(0, 5);

    // below-min → [0, 1, 1]
    expect(out[3]).toBeCloseTo(0, 5);
    expect(out[4]).toBeCloseTo(1, 5);
    expect(out[5]).toBeCloseTo(1, 5);

    // above-max → [1, 0, 1]
    expect(out[6]).toBeCloseTo(1, 5);
    expect(out[7]).toBeCloseTo(0, 5);
    expect(out[8]).toBeCloseTo(1, 5);
  });

  it('single-allocation contract: byteOffset is 0 and buffer is not over-allocated', () => {
    const out = bakeColours(scalars, range, 'viridis');
    // byteOffset === 0 confirms out is not a sliced view into a larger buffer.
    expect(out.byteOffset).toBe(0);
    // buffer.byteLength === out.byteLength confirms no slack allocation.
    expect(out.buffer.byteLength).toBe(out.byteLength);
  });

  it('empty scalars array returns empty Float32Array without throwing', () => {
    const out = bakeColours(new Float32Array(0), range, 'viridis');
    expect(out).toBeInstanceOf(Float32Array);
    expect(out.length).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// Step 9 — barrel-export wiring through gui/src/viewport/index.ts
// ---------------------------------------------------------------------------
describe('barrel export wiring (viewport/index)', () => {
  // The first cold import of the barrel triggers Solid JSX transformation of
  // Viewport.tsx → FeaModeToolbar.tsx (added in task 2961) and a ~2.3 s
  // module graph load. Global testTimeout (15 000 ms) and hookTimeout
  // (30 000 ms) in gui/vitest.config.ts provide the necessary headroom —
  // per-test overrides are no longer needed (task 3185 consolidation).
  type BarrelModule = typeof import('../../viewport/index');
  let barrel: BarrelModule;
  beforeAll(async () => {
    barrel = await import('../../viewport/index');
  });

  it('applyColormap is re-exported from the viewport barrel', () => {
    expect(typeof barrel.applyColormap).toBe('function');
  });

  it('bakeColours is re-exported from the viewport barrel', () => {
    expect(typeof barrel.bakeColours).toBe('function');
  });

  it('applyColormap returns a proper 3-element Array for all palettes and range modes', () => {
    const r: Range = { mode: 'fixed', min: 0, max: 1 };

    // All three palettes dispatch through the barrel correctly.
    for (const p of ['viridis', 'magma', 'rainbow'] as const) {
      const result = barrel.applyColormap(0.5, r, p);
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(3);
    }

    // All three Range mode variants are accepted.
    const autoR   = { mode: 'auto'   as const, min: 0, max: 1 };
    const lockedR = { mode: 'locked' as const, min: 0, max: 1, source: 'test' };
    for (const range of [autoR, r, lockedR]) {
      const result = barrel.applyColormap(0.5, range, 'viridis');
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(3);
    }

    // nanColor propagates through the barrel — the unique assertion not covered
    // by the preceding 'function is re-exported' tests.
    const opts: import('../../viewport/colormap').ColormapOptions = { nanColor: [0, 0, 1] };
    const nanResult = barrel.applyColormap(NaN, r, 'viridis', opts);
    expect(nanResult).toEqual([0, 0, 1]);
  });
});
