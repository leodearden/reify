import { describe, it, expect } from 'vitest';
import { formatDisplayNumber, convertToUnit, ladderForDimension } from '../stores/unitLadder';

describe('formatDisplayNumber', () => {
  it('formats a plain decimal verbatim', () => {
    expect(formatDisplayNumber(7.04500224)).toBe('7.04500224');
  });

  it('trims 12-sig-fig float noise from a division result', () => {
    expect(formatDisplayNumber(7045002.24 / 1e6)).toBe('7.04500224');
  });

  it('kills classic 0.1 + 0.2 float noise', () => {
    expect(formatDisplayNumber(0.1 + 0.2)).toBe('0.3');
  });

  it('formats a whole number without a decimal point', () => {
    expect(formatDisplayNumber(80)).toBe('80');
  });

  it('formats a negative decimal', () => {
    expect(formatDisplayNumber(-3.5)).toBe('-3.5');
  });

  it('takes the whole-number path for a large integral value', () => {
    expect(formatDisplayNumber(1e15)).toBe('1000000000000000');
  });

  it('falls back to String(v) for non-finite values', () => {
    expect(formatDisplayNumber(Infinity)).toBe(String(Infinity));
    expect(formatDisplayNumber(NaN)).toBe(String(NaN));
  });
});

describe('convertToUnit', () => {
  it('converts an SI volume magnitude to litres', () => {
    expect(convertToUnit(0.00704500224, 1e-3)).toBeCloseTo(7.04500224, 9);
  });

  it('converts an SI length magnitude to millimetres', () => {
    expect(convertToUnit(0.08, 1e-3)).toBeCloseTo(80, 9);
  });
});

describe('ladderForDimension', () => {
  const map = {
    Volume: [
      { label: 'mm³', si_scale: 1e-9, is_default: true },
      { label: 'L', si_scale: 1e-3, is_default: false },
    ],
  };

  it('returns the option list for a known dimension', () => {
    expect(ladderForDimension(map, 'Volume')).toBe(map.Volume);
  });

  it('returns undefined for an unknown dimension', () => {
    expect(ladderForDimension(map, 'Force')).toBeUndefined();
  });
});
