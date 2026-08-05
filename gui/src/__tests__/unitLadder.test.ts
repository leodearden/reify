import { describe, it, expect } from 'vitest';
import {
  formatDisplayNumber,
  convertToUnit,
  ladderForDimension,
  normalizeUnitLabel,
} from '../stores/unitLadder';

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

/**
 * Task #6028: the ASCII normal form of a curated unit label.
 *
 * Superscripts are written as `²`/`³` escapes here (mirroring the
 * escape convention on the Rust side, task #5788 addendum L2) so the
 * expectations stay greppable and cannot be silently mangled by an editor or
 * a locale-dependent normalisation pass.
 */
describe('normalizeUnitLabel', () => {
  it.each([
    ['mm²', 'mm^2'],
    ['cm²', 'cm^2'],
    ['m²', 'm^2'],
  ])('rewrites U+00B2 to ^2: %s -> %s', (input, expected) => {
    expect(normalizeUnitLabel(input)).toBe(expected);
  });

  it.each([
    ['mm³', 'mm^3'],
    ['cm³', 'cm^3'],
    ['m³', 'm^3'],
    ['kg/m³', 'kg/m^3'],
    ['g/cm³', 'g/cm^3'],
  ])('rewrites U+00B3 to ^3: %s -> %s', (input, expected) => {
    expect(normalizeUnitLabel(input)).toBe(expected);
  });

  it.each([['mm'], ['L'], ['deg'], ['rad'], ['kPa'], ['kg/m^3'], ['mm^3'], ['']])(
    'is the identity on a label carrying no superscript: %s',
    (input) => {
      expect(normalizeUnitLabel(input)).toBe(input);
    },
  );

  it.each([
    ['mm³'],
    ['kg/m³'],
    ['m²'],
    ['mm'],
    ['L'],
  ])('is idempotent: normalize(normalize(%s)) === normalize(%s)', (input) => {
    const once = normalizeUnitLabel(input);
    expect(normalizeUnitLabel(once)).toBe(once);
  });

  it('replaces EVERY occurrence, not just the first', () => {
    expect(normalizeUnitLabel('m³/m³')).toBe('m^3/m^3');
    expect(normalizeUnitLabel('m²·m³')).toBe('m^2·m^3');
  });

  it('rewrites ONLY the two exponent glyphs — nothing else is special-cased', () => {
    // A middle dot is structural in a compound label and must survive verbatim.
    expect(normalizeUnitLabel('N·m')).toBe('N·m');
    // No special case for engineering notation either: this is a pure glyph
    // substitution, so a magnitude string would have its exponent rewritten
    // too. That is exactly why callers must only feed it UNIT labels — the
    // `x10^n` superscript digits in `reify-ir` are magnitude formatting and
    // are out of scope (task #5788 PRD §10 / addendum L3).
    expect(normalizeUnitLabel('×10³')).toBe('×10^3');
    // Superscript digits OTHER than 2 and 3 (U+00B9 one, U+2074 four) are not
    // curated unit-label glyphs and are deliberately left alone.
    expect(normalizeUnitLabel('m¹')).toBe('m¹');
    expect(normalizeUnitLabel('m⁴')).toBe('m⁴');
  });
});
