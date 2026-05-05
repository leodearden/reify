import { describe, it, expect } from 'vitest';
import { viridisLut, magmaLut, rainbowLut } from '../../viewport/colormap';

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

  it('viridisLut entry 0 matches matplotlib reference (0.267004, 0.004874, 0.329415) within 1e-3', () => {
    expect(viridisLut[0]).toBeCloseTo(0.267004, 2);
    expect(viridisLut[1]).toBeCloseTo(0.004874, 2);
    expect(viridisLut[2]).toBeCloseTo(0.329415, 2);
  });

  it('viridisLut entry 255 matches matplotlib reference (0.993248, 0.906157, 0.143936) within 1e-3', () => {
    expect(viridisLut[255 * 3 + 0]).toBeCloseTo(0.993248, 2);
    expect(viridisLut[255 * 3 + 1]).toBeCloseTo(0.906157, 2);
    expect(viridisLut[255 * 3 + 2]).toBeCloseTo(0.143936, 2);
  });

  it('magmaLut entry 0 matches matplotlib reference (0.001462, 0.000466, 0.013866) within 1e-3', () => {
    expect(magmaLut[0]).toBeCloseTo(0.001462, 2);
    expect(magmaLut[1]).toBeCloseTo(0.000466, 2);
    expect(magmaLut[2]).toBeCloseTo(0.013866, 2);
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
