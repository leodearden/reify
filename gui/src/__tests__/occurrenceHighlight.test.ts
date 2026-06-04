import { describe, it, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import {
  highlightsToRanges,
  occurrenceHighlightField,
  setOccurrencesEffect,
} from '../editor/occurrenceHighlight';

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

// --- step-11/12: decoration StateField + setOccurrencesEffect (real CodeMirror) ---

/** Collect (from, to, class) of every mark in the field's DecorationSet. */
function marksOf(state: EditorState) {
  const decos = state.field(occurrenceHighlightField);
  const out: { from: number; to: number; cls: string | undefined }[] = [];
  const iter = decos.iter();
  while (iter.value !== null) {
    out.push({ from: iter.from, to: iter.to, cls: iter.value.spec.class });
    iter.next();
  }
  return out;
}

describe('occurrenceHighlightField', () => {
  const doc = 'structure Bracket {\n  let volume = width * width\n}';

  it('holds no decorations initially', () => {
    const state = EditorState.create({ doc, extensions: [occurrenceHighlightField] });
    expect(marksOf(state).length).toBe(0);
  });

  it('builds cm-occurrenceHighlight marks at the effect ranges', () => {
    const state = EditorState.create({ doc, extensions: [occurrenceHighlightField] });
    const next = state.update({
      effects: setOccurrencesEffect.of([
        { from: 22, to: 27 },
        { from: 30, to: 35 },
      ]),
    }).state;

    expect(marksOf(next)).toEqual([
      { from: 22, to: 27, cls: 'cm-occurrenceHighlight' },
      { from: 30, to: 35, cls: 'cm-occurrenceHighlight' },
    ]);
  });

  it('clears all decorations on an empty effect', () => {
    const withMarks = EditorState.create({
      doc,
      extensions: [occurrenceHighlightField],
    }).update({ effects: setOccurrencesEffect.of([{ from: 22, to: 27 }]) }).state;
    expect(marksOf(withMarks).length).toBe(1);

    const cleared = withMarks.update({ effects: setOccurrencesEffect.of([]) }).state;
    expect(marksOf(cleared).length).toBe(0);
  });
});
