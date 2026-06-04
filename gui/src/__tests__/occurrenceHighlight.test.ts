import { describe, it, expect } from 'vitest';
import { highlightsToRanges } from '../editor/occurrenceHighlight';

// Doc-like mock in the diagnostics.test.ts style. LSP lines are 0-based;
// doc.line(n) is 1-based, so LSP line L maps to doc.line(L + 1).
//   line 1 (LSP 0): [0, 20]; line 2 (LSP 1): [21, 40]; line 8 (LSP 7): [100, 130].
const mockDoc = {
  line: (n: number) => {
    if (n === 1) return { from: 0, to: 20 };
    if (n === 2) return { from: 21, to: 40 };
    if (n === 8) return { from: 100, to: 130 };
    throw new Error(`line ${n} out of range`);
  },
};

/** Build a DocumentHighlight-shaped object for the given LSP range. */
const hl = (sl: number, sc: number, el: number, ec: number) => ({
  range: { start: { line: sl, character: sc }, end: { line: el, character: ec } },
  kind: 1,
});

describe('highlightsToRanges', () => {
  it('maps an LSP highlight range to a CM {from,to} offset', () => {
    const result = highlightsToRanges([hl(0, 5, 0, 10)], mockDoc as any);
    expect(result).toEqual([{ from: 5, to: 10 }]);
  });

  it('maps multiple highlights across different lines', () => {
    const result = highlightsToRanges([hl(0, 5, 0, 10), hl(7, 17, 7, 22)], mockDoc as any);
    expect(result).toEqual([
      { from: 5, to: 10 }, // line 1 from 0 + (5, 10)
      { from: 117, to: 122 }, // line 8 from 100 + (17, 22)
    ]);
  });

  it('clamps an over-long end character to the line end (Math.min with line.to)', () => {
    // start char 5 → from 5; end char 999 exceeds line 1's length → clamp to line.to (20).
    const result = highlightsToRanges([hl(0, 5, 0, 999)], mockDoc as any);
    expect(result).toEqual([{ from: 5, to: 20 }]);
  });

  it('drops a degenerate range that clamps to zero width', () => {
    // Both start and end characters exceed the line → from === to === 20 → dropped.
    const result = highlightsToRanges([hl(0, 999, 0, 999)], mockDoc as any);
    expect(result).toEqual([]);
  });

  it('returns [] for empty input', () => {
    expect(highlightsToRanges([], mockDoc as any)).toEqual([]);
  });
});
