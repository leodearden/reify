/**
 * Unit tests for gui/src/editor/lspRange.ts.
 *
 * Mock doc mirrors the occurrenceHighlight.test.ts style:
 *   line(n) → {from, to} for valid 1-based lines; throws for out-of-range.
 *   line 1 (LSP 0): [0, 20]; line 2 (LSP 1): [21, 40].
 *
 * LSP positions are 0-based; CodeMirror doc.line() is 1-based,
 * so LSP line L → doc.line(L + 1).
 */
import { describe, it, expect } from 'vitest';
import { lspPositionToOffset, lspRangeToCmRange } from '../editor/lspRange';

const mockDoc = {
  line: (n: number) => {
    if (n === 1) return { from: 0, to: 20 };
    if (n === 2) return { from: 21, to: 40 };
    throw new RangeError(`line ${n} out of range`);
  },
};

describe('lspPositionToOffset', () => {
  it('maps a normal 0-based LSP position to line.from + character', () => {
    // LSP line 0, char 5 → doc.line(1).from (0) + 5 = 5
    expect(lspPositionToOffset(mockDoc as any, 0, 5)).toBe(5);
  });

  it('maps a position on line 1 correctly', () => {
    // LSP line 1, char 3 → doc.line(2).from (21) + 3 = 24
    expect(lspPositionToOffset(mockDoc as any, 1, 3)).toBe(24);
  });

  it('clamps an over-long character to line.to', () => {
    // LSP line 0, char 999 → doc.line(1): from 0, to 20 → min(0+999, 20) = 20
    expect(lspPositionToOffset(mockDoc as any, 0, 999)).toBe(20);
  });

  it('THROWS for an out-of-range line (propagates doc.line RangeError)', () => {
    // LSP line 50 → doc.line(51) which throws
    expect(() => lspPositionToOffset(mockDoc as any, 50, 0)).toThrow();
  });
});

describe('lspRangeToCmRange', () => {
  it('maps a normal LSP range to a CmRange with both endpoints clamped', () => {
    // start: line 0, char 5 → 5; end: line 0, char 10 → 10
    const result = lspRangeToCmRange(mockDoc as any, {
      start: { line: 0, character: 5 },
      end: { line: 0, character: 10 },
    });
    expect(result).toEqual({ from: 5, to: 10 });
  });

  it('clamps both endpoints when characters are over-long', () => {
    // start: line 0, char 999 → 20; end: line 1, char 999 → 40
    const result = lspRangeToCmRange(mockDoc as any, {
      start: { line: 0, character: 999 },
      end: { line: 1, character: 999 },
    });
    expect(result).toEqual({ from: 20, to: 40 });
  });

  it('returns null (does NOT throw) when the start line is out of range', () => {
    expect(() =>
      lspRangeToCmRange(mockDoc as any, {
        start: { line: 50, character: 0 },
        end: { line: 50, character: 5 },
      }),
    ).not.toThrow();
    expect(
      lspRangeToCmRange(mockDoc as any, {
        start: { line: 50, character: 0 },
        end: { line: 50, character: 5 },
      }),
    ).toBeNull();
  });

  it('returns null (does NOT throw) when the end line is out of range', () => {
    // Start is valid but end is out of range
    expect(() =>
      lspRangeToCmRange(mockDoc as any, {
        start: { line: 0, character: 0 },
        end: { line: 50, character: 5 },
      }),
    ).not.toThrow();
    expect(
      lspRangeToCmRange(mockDoc as any, {
        start: { line: 0, character: 0 },
        end: { line: 50, character: 5 },
      }),
    ).toBeNull();
  });

  it('PRESERVES a degenerate/zero-width range (from === to after clamp) — does NOT drop it', () => {
    // start: line 0, char 999 → 20; end: line 0, char 999 → 20
    // Produces {from:20, to:20} — a zero-width range must be returned, NOT null.
    const result = lspRangeToCmRange(mockDoc as any, {
      start: { line: 0, character: 999 },
      end: { line: 0, character: 999 },
    });
    expect(result).not.toBeNull();
    expect(result).toEqual({ from: 20, to: 20 });
  });

  it('preserves an explicitly zero-width range (insertion point)', () => {
    // Pure insertion: start === end at a normal position
    const result = lspRangeToCmRange(mockDoc as any, {
      start: { line: 0, character: 5 },
      end: { line: 0, character: 5 },
    });
    expect(result).toEqual({ from: 5, to: 5 });
  });
});
