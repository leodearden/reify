/**
 * Tests for the pure computeModeThumbnail projection helper (task 4072, step-9).
 *
 * computeModeThumbnail(base, peak) projects each mode's peak node positions
 * onto the two largest-extent base-bbox axes and normalises into a [0,1]²
 * viewBox. It is a pure function with no DOM or WebGL dependencies.
 *
 * RED at step-9: the module does not exist yet.
 * GREEN after step-10 adds gui/src/viewport/modeThumbnail.ts.
 */

import { describe, it, expect } from 'vitest';
import { computeModeThumbnail } from '../../viewport/modeThumbnail';

// 3 nodes: (0,0,0), (1,0,0), (0,1,0) — extent X=1, Y=1, Z=0
const BASE3 = [0, 0, 0, 1, 0, 0, 0, 1, 0];
// Peak: each node displaced by 0.1 in X
const PEAK3 = [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0];

describe('computeModeThumbnail', () => {
  it('returns one [x,y] point per node (base.length / 3)', () => {
    const { points } = computeModeThumbnail(BASE3, PEAK3);
    expect(points).toHaveLength(3); // 3 nodes → 3 points
  });

  it('every point coordinate is finite and within [0, 1]', () => {
    const { points } = computeModeThumbnail(BASE3, PEAK3);
    for (const [x, y] of points) {
      expect(Number.isFinite(x)).toBe(true);
      expect(Number.isFinite(y)).toBe(true);
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThanOrEqual(1);
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(1);
    }
  });

  it('returns viewBox "0 0 1 1"', () => {
    const { viewBox } = computeModeThumbnail(BASE3, PEAK3);
    expect(viewBox).toBe('0 0 1 1');
  });

  it('result is deterministic for fixed input', () => {
    const a = computeModeThumbnail(BASE3, PEAK3);
    const b = computeModeThumbnail(BASE3, PEAK3);
    expect(a.points).toEqual(b.points);
    expect(a.viewBox).toBe(b.viewBox);
  });

  it('degenerate base (all nodes equal, zero-extent bbox) yields no NaN — falls back to 0.5', () => {
    const degBase = [1, 1, 1, 1, 1, 1, 1, 1, 1];
    const degPeak = [1.1, 1, 1, 1.1, 1, 1, 1.1, 1, 1];
    const { points } = computeModeThumbnail(degBase, degPeak);
    expect(points).toHaveLength(3);
    for (const [x, y] of points) {
      expect(Number.isNaN(x)).toBe(false);
      expect(Number.isNaN(y)).toBe(false);
    }
  });

  it('single-node input (base.length === 3) does not throw', () => {
    const { points } = computeModeThumbnail([0, 0, 0], [1, 1, 1]);
    expect(points).toHaveLength(1);
    for (const [x, y] of points) {
      expect(Number.isFinite(x)).toBe(true);
      expect(Number.isFinite(y)).toBe(true);
    }
  });
});
