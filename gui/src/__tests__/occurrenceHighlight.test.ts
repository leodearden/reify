import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import {
  highlightsToRanges,
  occurrenceHighlightField,
  occurrenceHighlightExtension,
  setOccurrencesEffect,
} from '../editor/occurrenceHighlight';
import type { DocumentHighlight } from '../editor/lspClient';
import { deferred } from './test-utils';

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

  it('drops an out-of-range highlight instead of throwing', () => {
    // doc.line() throws a RangeError for a line past the document (real
    // CodeMirror Text.line() behavior, mirrored by mockDoc). A stale /
    // version-skewed response can carry such a line; it must degrade to a
    // dropped mark, not throw out of the dispatch path.
    expect(() => highlightsToRanges([hl(50, 0, 50, 3)], mockDoc as any)).not.toThrow();
    expect(highlightsToRanges([hl(50, 0, 50, 3)], mockDoc as any)).toEqual([]);
  });

  it('keeps in-range highlights even when another highlight is out of range', () => {
    // One bad mark must not poison the rest — each highlight is guarded
    // independently so the in-range occurrence still paints.
    const result = highlightsToRanges([hl(0, 5, 0, 10), hl(50, 0, 50, 3)], mockDoc as any);
    expect(result).toEqual([{ from: 5, to: 10 }]);
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

// --- step-13/14: occurrenceHighlightExtension ViewPlugin (real jsdom EditorView) ---

const DEBOUNCE = 150;

/** Mount a standalone EditorView in jsdom with the given extension. */
function mountView(docText: string, ext: Extension): EditorView {
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  return new EditorView({
    state: EditorState.create({ doc: docText, extensions: [ext] }),
    parent,
  });
}

describe('occurrenceHighlightExtension', () => {
  // doc 'abc\ndefgh': line 1 'abc' [0,3]; line 2 'defgh' [4,9].
  // Cursor offset 5 → CM line 2 → LSP line 1, char 1.
  const docText = 'abc\ndefgh';

  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('clears existing decorations synchronously on a cursor move', () => {
    const client = { documentHighlight: vi.fn(async () => [] as DocumentHighlight[]) };
    const view = mountView(docText, occurrenceHighlightExtension(() => 'file:///a.ri', client, DEBOUNCE));

    // Pre-populate marks, then move the cursor.
    view.dispatch({ effects: setOccurrencesEffect.of([{ from: 4, to: 7 }]) });
    expect(marksOf(view.state).length).toBe(1);

    view.dispatch({ selection: { anchor: 5 } });
    // Cleared in the SAME transaction — before any debounce timer fires.
    expect(marksOf(view.state).length).toBe(0);
    view.destroy();
  });

  it('requests documentHighlight only after the debounce, at the cursor 0-based line/char', async () => {
    const client = { documentHighlight: vi.fn(async () => [] as DocumentHighlight[]) };
    const view = mountView(docText, occurrenceHighlightExtension(() => 'file:///a.ri', client, DEBOUNCE));

    view.dispatch({ selection: { anchor: 5 } });
    expect(client.documentHighlight).not.toHaveBeenCalled();

    vi.advanceTimersByTime(DEBOUNCE - 1);
    expect(client.documentHighlight).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(client.documentHighlight).toHaveBeenCalledTimes(1);
    expect(client.documentHighlight).toHaveBeenCalledWith('file:///a.ri', 1, 1);
    view.destroy();
  });

  it('paints the mapped decorations after the response resolves', async () => {
    const client = { documentHighlight: vi.fn(async () => [hl(1, 0, 1, 3)] as DocumentHighlight[]) };
    const view = mountView(docText, occurrenceHighlightExtension(() => 'file:///a.ri', client, DEBOUNCE));

    view.dispatch({ selection: { anchor: 5 } });
    await vi.advanceTimersByTimeAsync(DEBOUNCE);

    // hl(1,0,1,3) → doc.line(2).from (4) + (0,3) = {from:4,to:7}.
    expect(marksOf(view.state)).toEqual([{ from: 4, to: 7, cls: 'cm-occurrenceHighlight' }]);
    view.destroy();
  });

  it('a second cursor move within the debounce window cancels the first request', async () => {
    const client = { documentHighlight: vi.fn(async () => [] as DocumentHighlight[]) };
    const view = mountView(docText, occurrenceHighlightExtension(() => 'file:///a.ri', client, DEBOUNCE));

    view.dispatch({ selection: { anchor: 5 } });
    vi.advanceTimersByTime(DEBOUNCE - 10); // first timer not yet fired
    view.dispatch({ selection: { anchor: 2 } }); // resets the debounce
    await vi.advanceTimersByTimeAsync(DEBOUNCE);

    expect(client.documentHighlight).toHaveBeenCalledTimes(1);
    view.destroy();
  });

  it('discards the response if the document URI changed while the request was in flight', async () => {
    let uri = 'file:///a.ri';
    const d = deferred<DocumentHighlight[]>();
    const client = { documentHighlight: vi.fn(() => d.promise) };
    const view = mountView(docText, occurrenceHighlightExtension(() => uri, client, DEBOUNCE));

    view.dispatch({ selection: { anchor: 5 } });
    vi.advanceTimersByTime(DEBOUNCE); // fire timer → request starts, captures uri='file:///a.ri'
    expect(client.documentHighlight).toHaveBeenCalledTimes(1);

    // File switch lands before the in-flight response resolves.
    uri = 'file:///b.ri';
    d.resolve([hl(1, 0, 1, 3)]);
    await vi.advanceTimersByTimeAsync(1);

    // Stale URI → result must be dropped, no decorations painted into the wrong buffer.
    expect(marksOf(view.state).length).toBe(0);
    view.destroy();
  });
});
