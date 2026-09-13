/**
 * Unit tests for the F2 inline-rename editor glue (gui/src/editor/rename.ts).
 *
 * Modeled on gotoDefinition.test.ts: a minimal mock EditorView (no DOM / no real
 * CodeMirror) drives the pure routing + offset-mapping logic.  The LSP client and
 * the inline-field UI are injected, so the refuse/accept routing is verifiable
 * without DOM or CM layout.
 *
 * applyWorkspaceEdit — converts an LSP WorkspaceEdit's per-URI TextEdits into a
 * SINGLE CodeMirror view.dispatch (multi-change), mapping each LSP range to a CM
 * offset via doc.line(line + 1).from + character.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { EditorView } from '@codemirror/view';
import { applyWorkspaceEdit, applyTextEditsToString, applyWorkspaceEditAcrossFiles, renameCommand, workspaceEditTargets, staleEditTargets } from '../editor/rename';
import type { RenameClient, RenameUi } from '../editor/rename';
import type { WorkspaceEdit } from '../editor/lspClient';
import { flushMacrotasks } from './test-utils';

beforeEach(() => {
  vi.clearAllMocks();
});

/**
 * Factory for a minimal mock EditorView used by the rename tests.
 *
 * doc.line(n) maps 1-based line n → { from: (n-1)*20, to: (n-1)*20 + 15 } so the
 * offset arithmetic in assertions is easy to follow (line L 0-based → from = L*20).
 * lineAt / selection / dom defaults support the renameCommand tests (step-13).
 */
function makeMockView(overrides?: {
  dispatch?: ReturnType<typeof vi.fn>;
  state?: {
    doc?: {
      line?: (n: number) => { from: number; to: number };
      lineAt?: (pos: number) => { number: number; from: number; to: number };
      lines?: number;
      length?: number;
    };
    selection?: { main?: { head?: number } };
  };
  dom?: { isConnected?: boolean };
}) {
  return {
    state: {
      doc: {
        line:
          overrides?.state?.doc?.line ??
          ((n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 })),
        lineAt:
          overrides?.state?.doc?.lineAt ??
          ((_pos: number) => ({ number: 1, from: 0, to: 10 })),
        lines: overrides?.state?.doc?.lines ?? 100,
        length: overrides?.state?.doc?.length ?? 2000,
      },
      selection: {
        main: { head: overrides?.state?.selection?.main?.head ?? 5 },
      },
    },
    dispatch: overrides?.dispatch ?? vi.fn(),
    dom: { isConnected: overrides?.dom?.isConnected ?? true },
  } as unknown as EditorView;
}

const URI = 'file:///test.ri';

// ---------------------------------------------------------------------------
// applyTextEditsToString — pure string transform, no CodeMirror/Tauri
// ---------------------------------------------------------------------------

describe('applyTextEditsToString', () => {
  // LSP TextEdit = { range: { start: { line, character }, end: { line, character } }, newText }
  // Lines are 0-based; characters are 0-based column offsets.

  it('applies a single edit replacing a substring on one line', () => {
    // source: "hello world\n"
    // edit: line 0, char 6..11 ("world") → "reify"
    const source = 'hello world\n';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 6 }, end: { line: 0, character: 11 } }, newText: 'reify' },
    ]);
    expect(result).toBe('hello reify\n');
  });

  it('applies multiple non-overlapping ascending edits without offset drift (right-to-left)', () => {
    // source: "struct Foo { sub x: Bar }\n"
    //          0123456789012345678901234
    //                    1111111111222222
    // edit 1: line 0, char 7..10 ("Foo") → "Baz"
    // edit 2: line 0, char 20..23 ("Bar") → "Baz"
    //   ('B'=20, 'a'=21, 'r'=22, ' '=23)
    // Both should be replaced independently.
    const source = 'struct Foo { sub x: Bar }\n';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 7 }, end: { line: 0, character: 10 } }, newText: 'Baz' },
      { range: { start: { line: 0, character: 20 }, end: { line: 0, character: 23 } }, newText: 'Baz' },
    ]);
    expect(result).toBe('struct Baz { sub x: Baz }\n');
  });

  it('applies a multi-line edit replacing content across lines', () => {
    // source: "line0\nline1\nline2\n"
    // edit: line 0 char 0 → line 1 char 5 (covers "line0\nline1") → "replaced"
    const source = 'line0\nline1\nline2\n';
    const result = applyTextEditsToString(source, [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 1, character: 5 } },
        newText: 'replaced',
      },
    ]);
    expect(result).toBe('replaced\nline2\n');
  });

  it('applies edits in descending offset order even when provided ascending', () => {
    // Three edits in ascending order; right-to-left application must prevent offset drift.
    // source: "aaa bbb ccc"
    // edit 1: char 0..3 ("aaa") → "AAA"
    // edit 2: char 4..7 ("bbb") → "BBB"
    // edit 3: char 8..11 ("ccc") → "CCC"
    const source = 'aaa bbb ccc';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } }, newText: 'AAA' },
      { range: { start: { line: 0, character: 4 }, end: { line: 0, character: 7 } }, newText: 'BBB' },
      { range: { start: { line: 0, character: 8 }, end: { line: 0, character: 11 } }, newText: 'CCC' },
    ]);
    expect(result).toBe('AAA BBB CCC');
  });

  it('handles a pure-insertion edit (zero-width range)', () => {
    // Insert "X" at position (0, 5) without replacing anything.
    const source = 'hello world';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 5 }, end: { line: 0, character: 5 } }, newText: 'X' },
    ]);
    expect(result).toBe('helloX world');
  });

  it('handles a pure-deletion edit (empty newText)', () => {
    // Delete chars 5..10 (inclusive) — removes " worl"
    const source = 'hello world';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } }, newText: '' },
    ]);
    expect(result).toBe('hellod');
  });

  it('returns the source unchanged when the edit list is empty', () => {
    const source = 'no change here';
    expect(applyTextEditsToString(source, [])).toBe(source);
  });

  it('clamps an out-of-range character to the line end (defense-in-depth)', () => {
    // source: "abc\n", line 0 has length 3 (chars 0..2 + newline at 3).
    // Edit with character 99 past end: should clamp to end of line.
    const source = 'abc\n';
    const result = applyTextEditsToString(source, [
      { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 99 } }, newText: 'X' },
    ]);
    // character 99 clamps to end of 'abc' (char 3), so the whole 'abc' is replaced by 'X'
    // and the trailing newline is preserved.  Exact expected string: 'X\n'.
    expect(result).toBe('X\n');
  });
});

// ---------------------------------------------------------------------------
// applyWorkspaceEditAcrossFiles — routing orchestrator (DI mocks, no I/O)
// ---------------------------------------------------------------------------

const ACTIVE_URI = 'file:///proj/main.ri';
const OPEN_URI = 'file:///proj/lib.ri';
const CLOSED_URI = 'file:///proj/other.ri';

const EDIT_A = [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } }, newText: 'AAA' }];
const EDIT_B = [{ range: { start: { line: 1, character: 0 }, end: { line: 1, character: 3 } }, newText: 'BBB' }];
const EDIT_C = [{ range: { start: { line: 2, character: 0 }, end: { line: 2, character: 3 } }, newText: 'CCC' }];

function makeDeps(openUris: string[] = [ACTIVE_URI, OPEN_URI]) {
  const applyActive = vi.fn();
  const applyOpenInactive = vi.fn();
  const applyClosed = vi.fn();
  const isOpen = vi.fn((uri: string) => openUris.includes(uri));
  return { applyActive, applyOpenInactive, applyClosed, isOpen };
}

// ---------------------------------------------------------------------------
// workspaceEditTargets — the single reader of the two WorkspaceEdit wire shapes
// ---------------------------------------------------------------------------

describe('workspaceEditTargets', () => {
  it('reads a documentChanges edit preserving order and per-entry version', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [
        { textDocument: { uri: OPEN_URI, version: 7 }, edits: EDIT_B },
        { textDocument: { uri: CLOSED_URI, version: null }, edits: EDIT_C },
      ],
    };
    expect(workspaceEditTargets(edit)).toEqual([
      { uri: OPEN_URI, version: 7, edits: EDIT_B },
      { uri: CLOSED_URI, version: null, edits: EDIT_C },
    ]);
  });

  it('reads a legacy changes map as one unversioned target per URI', () => {
    const edit: WorkspaceEdit = { changes: { [ACTIVE_URI]: EDIT_A, [CLOSED_URI]: EDIT_C } };
    expect(workspaceEditTargets(edit)).toEqual([
      { uri: ACTIVE_URI, version: null, edits: EDIT_A },
      { uri: CLOSED_URI, version: null, edits: EDIT_C },
    ]);
  });

  it('prefers documentChanges over changes when both are present (LSP precedence)', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [{ textDocument: { uri: OPEN_URI, version: 2 }, edits: EDIT_B }],
      changes: { [ACTIVE_URI]: EDIT_A },
    };
    expect(workspaceEditTargets(edit)).toEqual([
      { uri: OPEN_URI, version: 2, edits: EDIT_B },
    ]);
  });

  it('returns no targets for an edit carrying neither representation', () => {
    expect(workspaceEditTargets({})).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// staleEditTargets — the version-skew guard
// ---------------------------------------------------------------------------

describe('staleEditTargets', () => {
  /** A version reader over a fixed uri→version table. */
  const reader = (table: Record<string, number>) => (uri: string) => table[uri];

  const versioned = (uri: string, version: number | null): WorkspaceEdit => ({
    documentChanges: [{ textDocument: { uri, version }, edits: EDIT_A }],
  });

  it('flags a URI whose server version differs from the client version', () => {
    expect(
      staleEditTargets(versioned(ACTIVE_URI, 3), reader({ [ACTIVE_URI]: 5 })),
    ).toEqual([ACTIVE_URI]);
  });

  it('flags nothing when the versions agree', () => {
    expect(
      staleEditTargets(versioned(ACTIVE_URI, 3), reader({ [ACTIVE_URI]: 3 })),
    ).toEqual([]);
  });

  it('treats a null server version as not-stale (closed file, disk is master)', () => {
    expect(
      staleEditTargets(versioned(CLOSED_URI, null), reader({ [CLOSED_URI]: 9 })),
    ).toEqual([]);
  });

  it('treats an untracked client URI as not-stale (staleness is unknowable)', () => {
    expect(staleEditTargets(versioned(OPEN_URI, 4), reader({}))).toEqual([]);
  });

  it('flags only the mismatched URI of a multi-file edit', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [
        { textDocument: { uri: ACTIVE_URI, version: 2 }, edits: EDIT_A },
        { textDocument: { uri: OPEN_URI, version: 1 }, edits: EDIT_B },
      ],
    };
    expect(
      staleEditTargets(edit, reader({ [ACTIVE_URI]: 2, [OPEN_URI]: 6 })),
    ).toEqual([OPEN_URI]);
  });

  it('never flags a legacy unversioned changes edit', () => {
    const edit: WorkspaceEdit = { changes: { [ACTIVE_URI]: EDIT_A, [OPEN_URI]: EDIT_B } };
    expect(
      staleEditTargets(edit, reader({ [ACTIVE_URI]: 5, [OPEN_URI]: 6 })),
    ).toEqual([]);
  });
});

describe('applyWorkspaceEditAcrossFiles', () => {
  it('routes the active URI to applyActive', () => {
    const edit: WorkspaceEdit = { changes: { [ACTIVE_URI]: EDIT_A } };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).toHaveBeenCalledOnce();
    expect(deps.applyActive).toHaveBeenCalledWith(ACTIVE_URI, EDIT_A);
    expect(deps.applyOpenInactive).not.toHaveBeenCalled();
    expect(deps.applyClosed).not.toHaveBeenCalled();
  });

  it('routes an open-inactive URI to applyOpenInactive (isOpen returns true)', () => {
    const edit: WorkspaceEdit = { changes: { [OPEN_URI]: EDIT_B } };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyOpenInactive).toHaveBeenCalledOnce();
    expect(deps.applyOpenInactive).toHaveBeenCalledWith(OPEN_URI, EDIT_B);
    expect(deps.applyActive).not.toHaveBeenCalled();
    expect(deps.applyClosed).not.toHaveBeenCalled();
  });

  it('routes a closed URI to applyClosed (isOpen returns false)', () => {
    const edit: WorkspaceEdit = { changes: { [CLOSED_URI]: EDIT_C } };
    const deps = makeDeps(); // CLOSED_URI not in open set
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyClosed).toHaveBeenCalledOnce();
    expect(deps.applyClosed).toHaveBeenCalledWith(CLOSED_URI, EDIT_C);
    expect(deps.applyActive).not.toHaveBeenCalled();
    expect(deps.applyOpenInactive).not.toHaveBeenCalled();
  });

  it('routes all three URIs correctly in a multi-uri edit', () => {
    const edit: WorkspaceEdit = {
      changes: {
        [ACTIVE_URI]: EDIT_A,
        [OPEN_URI]: EDIT_B,
        [CLOSED_URI]: EDIT_C,
      },
    };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).toHaveBeenCalledOnce();
    expect(deps.applyActive).toHaveBeenCalledWith(ACTIVE_URI, EDIT_A);
    expect(deps.applyOpenInactive).toHaveBeenCalledOnce();
    expect(deps.applyOpenInactive).toHaveBeenCalledWith(OPEN_URI, EDIT_B);
    expect(deps.applyClosed).toHaveBeenCalledOnce();
    expect(deps.applyClosed).toHaveBeenCalledWith(CLOSED_URI, EDIT_C);
  });

  it('skips a URI with an empty edit list (no call to any sink)', () => {
    const edit: WorkspaceEdit = {
      changes: {
        [ACTIVE_URI]: [], // empty — must be skipped
        [CLOSED_URI]: EDIT_C,
      },
    };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).not.toHaveBeenCalled(); // empty list → skip
    expect(deps.applyClosed).toHaveBeenCalledWith(CLOSED_URI, EDIT_C);
  });

  it('does nothing when changes is absent', () => {
    const edit: WorkspaceEdit = {};
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).not.toHaveBeenCalled();
    expect(deps.applyOpenInactive).not.toHaveBeenCalled();
    expect(deps.applyClosed).not.toHaveBeenCalled();
  });

  it('does NOT call isOpen for the active URI (routing is by identity first)', () => {
    const edit: WorkspaceEdit = { changes: { [ACTIVE_URI]: EDIT_A } };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    // isOpen should NOT have been consulted for the active URI
    expect(deps.isOpen).not.toHaveBeenCalledWith(ACTIVE_URI);
  });

  // Task 7118: once the client declares the documentChanges capability the
  // server stops sending `changes` entirely. An applier that only reads
  // `changes` would become a silent no-op — rename reporting success while
  // editing nothing. These pin identical routing for the versioned shape.

  it('routes a documentChanges edit exactly like the equivalent changes edit', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [
        { textDocument: { uri: ACTIVE_URI, version: 4 }, edits: EDIT_A },
        { textDocument: { uri: OPEN_URI, version: 2 }, edits: EDIT_B },
        { textDocument: { uri: CLOSED_URI, version: null }, edits: EDIT_C },
      ],
    };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).toHaveBeenCalledOnce();
    expect(deps.applyActive).toHaveBeenCalledWith(ACTIVE_URI, EDIT_A);
    expect(deps.applyOpenInactive).toHaveBeenCalledOnce();
    expect(deps.applyOpenInactive).toHaveBeenCalledWith(OPEN_URI, EDIT_B);
    expect(deps.applyClosed).toHaveBeenCalledOnce();
    expect(deps.applyClosed).toHaveBeenCalledWith(CLOSED_URI, EDIT_C);
  });

  it('skips a documentChanges entry with an empty edit list', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [
        { textDocument: { uri: ACTIVE_URI, version: 4 }, edits: [] },
        { textDocument: { uri: CLOSED_URI, version: null }, edits: EDIT_C },
      ],
    };
    const deps = makeDeps();
    applyWorkspaceEditAcrossFiles(edit, ACTIVE_URI, deps);
    expect(deps.applyActive).not.toHaveBeenCalled();
    expect(deps.applyClosed).toHaveBeenCalledWith(CLOSED_URI, EDIT_C);
  });
});

describe('applyWorkspaceEdit', () => {
  it('dispatches ONE transaction mapping a single TextEdit to a CM change', () => {
    // range start/end on line 2 (0-based) → doc.line(3) = { from: 40, to: 55 }
    // from = 40 + 3 = 43, to = 40 + 8 = 48, insert = 'girth'
    const edit: WorkspaceEdit = {
      changes: {
        [URI]: [
          {
            range: { start: { line: 2, character: 3 }, end: { line: 2, character: 8 } },
            newText: 'girth',
          },
        ],
      },
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(true);
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: [{ from: 43, to: 48, insert: 'girth' }],
      userEvent: 'rename',
    });
  });

  it('maps multiple TextEdits into multiple change entries in ONE dispatch', () => {
    // edit 1: line 0 → doc.line(1) = { from: 0 }  → from 5, to 10
    // edit 2: line 2 → doc.line(3) = { from: 40 } → from 43, to 48
    const edit: WorkspaceEdit = {
      changes: {
        [URI]: [
          {
            range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
            newText: 'girth',
          },
          {
            range: { start: { line: 2, character: 3 }, end: { line: 2, character: 8 } },
            newText: 'girth',
          },
        ],
      },
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(true);
    // A SINGLE dispatch carrying BOTH changes (atomic, undo-able as one op).
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: [
        { from: 5, to: 10, insert: 'girth' },
        { from: 43, to: 48, insert: 'girth' },
      ],
      userEvent: 'rename',
    });
  });

  it('clamps an out-of-range edit character to the line end (defense-in-depth)', () => {
    // Mock line 0 → { from: 0, to: 15 } (length 15). A character of 30/40 exceeds
    // the line; the mapped offset must clamp to .to (15), never overflow past it.
    const edit: WorkspaceEdit = {
      changes: {
        [URI]: [
          {
            range: { start: { line: 0, character: 30 }, end: { line: 0, character: 40 } },
            newText: 'girth',
          },
        ],
      },
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(true);
    // from = min(0 + 30, 15) = 15 ; to = min(0 + 40, 15) = 15.
    expect(dispatch).toHaveBeenCalledWith({
      changes: [{ from: 15, to: 15, insert: 'girth' }],
      userEvent: 'rename',
    });
  });

  it('returns false and does NOT dispatch when the edit has no changes for the uri', () => {
    const edit: WorkspaceEdit = {
      changes: {
        'file:///other.ri': [
          {
            range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
            newText: 'girth',
          },
        ],
      },
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
  });

  it('returns false and does NOT dispatch when the edit has no `changes` map at all', () => {
    const edit: WorkspaceEdit = {};
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
  });

  it('returns false and does NOT dispatch when the uri maps to an empty edit list', () => {
    const edit: WorkspaceEdit = { changes: { [URI]: [] } };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
  });

  // --- NEW hardening test (RED against current unguarded impl) ---

  it('skips out-of-range edits without throwing; dispatches ONE transaction with only in-range edits', () => {
    // Line 999 is out of range in the mock doc (makeMockView maps line(n) for n up to
    // 100 with no throw, so override line to throw for n > 50).
    // In-range edit: line 0 → doc.line(1) = {from:0, to:15} → from 5, to 10.
    // Out-of-range edit: line 999 → doc.line(1000) throws RangeError.
    const dispatch = vi.fn();
    const view = makeMockView({
      dispatch,
      state: {
        doc: {
          line: (n: number) => {
            if (n > 50) throw new RangeError(`line ${n} out of range`);
            return { from: (n - 1) * 20, to: (n - 1) * 20 + 15 };
          },
        },
      },
    });

    const edit: WorkspaceEdit = {
      changes: {
        [URI]: [
          // In-range edit: line 0 char 5→10 → from 5, to 10
          {
            range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
            newText: 'girth',
          },
          // Out-of-range edit: line 999 → throws in doc.line → must be skipped
          {
            range: { start: { line: 999, character: 0 }, end: { line: 999, character: 5 } },
            newText: 'bad',
          },
        ],
      },
    };

    // A thrown error would fail the test; assert return value and dispatch
    // state on the single call without re-invoking.
    const result = applyWorkspaceEdit(view, edit, URI);

    // Returns true (at least one edit survived).
    expect(result).toBe(true);
    // Dispatched exactly once, carrying only the in-range edit.
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: [{ from: 5, to: 10, insert: 'girth' }],
      userEvent: 'rename',
    });
  });

  // Task 7118: the same silent-no-op regression pin for the single-uri applier.

  it('dispatches a documentChanges edit exactly like the equivalent changes edit', () => {
    const textEdit = {
      range: { start: { line: 2, character: 3 }, end: { line: 2, character: 8 } },
      newText: 'girth',
    };
    const edit: WorkspaceEdit = {
      documentChanges: [{ textDocument: { uri: URI, version: 9 }, edits: [textEdit] }],
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    const result = applyWorkspaceEdit(view, edit, URI);

    expect(result).toBe(true);
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: [{ from: 43, to: 48, insert: 'girth' }],
      userEvent: 'rename',
    });
  });

  it('returns false when a documentChanges edit carries no entry for the uri', () => {
    const edit: WorkspaceEdit = {
      documentChanges: [
        {
          textDocument: { uri: 'file:///other.ri', version: 1 },
          edits: [
            {
              range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
              newText: 'girth',
            },
          ],
        },
      ],
    };
    const dispatch = vi.fn();
    const view = makeMockView({ dispatch });

    expect(applyWorkspaceEdit(view, edit, URI)).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// renameCommand — CodeMirror Command factory (cursor → prepareRename → UI)
// ---------------------------------------------------------------------------

/**
 * Build a mocked rename client + ui + document-version reader from
 * individually-trackable vi.fns.
 *
 * `currentVersion` defaults to returning undefined (an untracked URI), which is
 * not-stale by construction — so a test only has to stub it when it is actually
 * exercising the version-skew guard.
 */
function makeRenameDeps() {
  const prepareRename = vi.fn();
  const rename = vi.fn();
  const promptNewName = vi.fn();
  const showCannotRename = vi.fn();
  const showRenameFailed = vi.fn();
  const currentVersion = vi.fn();
  const client = { prepareRename, rename } as unknown as RenameClient;
  const ui = { promptNewName, showCannotRename, showRenameFailed } as unknown as RenameUi;
  return {
    client,
    ui,
    prepareRename,
    rename,
    promptNewName,
    showCannotRename,
    showRenameFailed,
    currentVersion,
  };
}

describe('renameCommand', () => {
  it('(a) returns a CodeMirror Command (function) that returns true synchronously', () => {
    const { client, ui, prepareRename } = makeRenameDeps();
    prepareRename.mockResolvedValue(null);

    const command = renameCommand(() => URI, client, ui);
    expect(typeof command).toBe('function');

    const view = makeMockView();
    // Always consume the key — even before the async prepareRename resolves.
    expect(command(view)).toBe(true);
  });

  it('(d) derives 0-based line/char from view.state.selection.main.head', async () => {
    const { client, ui, prepareRename } = makeRenameDeps();
    prepareRename.mockResolvedValue(null);

    // head = 47; lineAt(47) → line 3 (1-based), from 40
    // lspLine = 3 - 1 = 2 ; lspChar = 47 - 40 = 7
    const view = makeMockView({
      state: {
        selection: { main: { head: 47 } },
        doc: { lineAt: (_pos: number) => ({ number: 3, from: 40, to: 55 }) },
      },
    });

    renameCommand(() => URI, client, ui)(view);
    await flushMacrotasks();

    expect(prepareRename).toHaveBeenCalledWith(URI, 2, 7);
  });

  it('(b) REFUSAL: prepareRename → null calls ui.showCannotRename and performs no edit', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showCannotRename } = makeRenameDeps();
    prepareRename.mockResolvedValue(null);

    const dispatch = vi.fn();
    // head = 5 ; lineAt(5) → line 1, from 0 → lspLine 0, lspChar 5
    const view = makeMockView({
      dispatch,
      state: {
        selection: { main: { head: 5 } },
        doc: { lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }) },
      },
    });

    const result = renameCommand(() => URI, client, ui)(view);
    expect(result).toBe(true);

    await flushMacrotasks();

    expect(prepareRename).toHaveBeenCalledWith(URI, 0, 5);
    // Refusal path: show the message, open NO field, request NO rename, edit NOTHING.
    expect(showCannotRename).toHaveBeenCalledTimes(1);
    expect(showCannotRename).toHaveBeenCalledWith(view);
    expect(promptNewName).not.toHaveBeenCalled();
    expect(rename).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();
  });

  it('(c) ACCEPT: opens the prompt; onSubmit triggers rename and applies the edit', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showCannotRename } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    const edit: WorkspaceEdit = {
      changes: { [URI]: [{ range: target.range, newText: 'girth' }] },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(edit);

    const dispatch = vi.fn();
    // head = 5 → lspLine 0, lspChar 5
    const view = makeMockView({
      dispatch,
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    renameCommand(() => URI, client, ui)(view);
    await flushMacrotasks();

    // Accept path: open the inline field with the target, refuse NOTHING.
    expect(prepareRename).toHaveBeenCalledWith(URI, 0, 5);
    expect(showCannotRename).not.toHaveBeenCalled();
    expect(promptNewName).toHaveBeenCalledTimes(1);
    const [promptView, promptRange, promptPlaceholder, onSubmit, onCancel] =
      promptNewName.mock.calls[0];
    expect(promptView).toBe(view);
    expect(promptRange).toEqual(target.range);
    expect(promptPlaceholder).toBe('width');
    expect(typeof onSubmit).toBe('function');
    expect(typeof onCancel).toBe('function');
    // No rename/edit happens until the user actually submits a new name.
    expect(rename).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();

    // Submitting "girth" requests the rename and applies the returned edit.
    onSubmit('girth');
    await flushMacrotasks();

    expect(rename).toHaveBeenCalledWith(URI, 0, 5, 'girth');
    // applyWorkspaceEdit mapped line 0 char 5..10 → from 5, to 10.
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: [{ from: 5, to: 10, insert: 'girth' }],
      userEvent: 'rename',
    });
  });

  it('does NOT apply the edit when the view was destroyed (isConnected=false) before rename resolved', async () => {
    const { client, ui, prepareRename, rename, promptNewName } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    const edit: WorkspaceEdit = {
      changes: { [URI]: [{ range: target.range, newText: 'girth' }] },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(edit);

    const dispatch = vi.fn();
    const view = makeMockView({
      dispatch,
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
      dom: { isConnected: false },
    });

    renameCommand(() => URI, client, ui)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    onSubmit('girth');
    await flushMacrotasks();

    // The rename request still fires, but the destroyed view must NOT be dispatched to.
    expect(rename).toHaveBeenCalledWith(URI, 0, 5, 'girth');
    expect(dispatch).not.toHaveBeenCalled();
  });

  it('post-submit refusal: rename → null shows showRenameFailed and dispatches nothing', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    prepareRename.mockResolvedValue(target);
    // Server rejects the accepted name (e.g. "2x" / "let") → Ok(None) → null.
    rename.mockResolvedValue(null);

    const dispatch = vi.fn();
    const view = makeMockView({
      dispatch,
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    renameCommand(() => URI, client, ui)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    onSubmit('2x');
    await flushMacrotasks();

    expect(rename).toHaveBeenCalledWith(URI, 0, 5, '2x');
    // No edit applied, but the user gets explicit feedback (not a silent drop).
    expect(dispatch).not.toHaveBeenCalled();
    expect(showRenameFailed).toHaveBeenCalledTimes(1);
    expect(showRenameFailed).toHaveBeenCalledWith(view);
  });

  it('file-switch race: a file change before rename resolves blocks the stale apply', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    const edit: WorkspaceEdit = {
      changes: { [URI]: [{ range: target.range, newText: 'girth' }] },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(edit);

    let currentUri = URI;
    const dispatch = vi.fn();
    const view = makeMockView({
      dispatch,
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    renameCommand(() => currentUri, client, ui)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    // User switches files while the inline field is open / rename is in flight.
    currentUri = 'file:///other.ri';
    onSubmit('girth');
    await flushMacrotasks();

    // The rename request still fires for the original file, but its edit must NOT
    // be applied onto the now-different buffer (and no failure message either).
    expect(rename).toHaveBeenCalledWith(URI, 0, 5, 'girth');
    expect(dispatch).not.toHaveBeenCalled();
    expect(showRenameFailed).not.toHaveBeenCalled();
  });

  it('file-switch race: a file change before prepareRename resolves opens no prompt', async () => {
    const { client, ui, prepareRename, promptNewName, showCannotRename } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    prepareRename.mockResolvedValue(target);

    let currentUri = URI;
    const view = makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: { lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }) },
      },
    });

    renameCommand(() => currentUri, client, ui)(view);
    // Switch files before prepareRename resolves.
    currentUri = 'file:///other.ri';
    await flushMacrotasks();

    // Stale prepareRename result must neither open the field nor show a refusal.
    expect(promptNewName).not.toHaveBeenCalled();
    expect(showCannotRename).not.toHaveBeenCalled();
  });

  it('uses the URI from the getter at call time (re-resolves after a file switch)', async () => {
    const { client, ui, prepareRename } = makeRenameDeps();
    prepareRename.mockResolvedValue(null);

    let currentUri = 'file:///first.ri';
    const command = renameCommand(() => currentUri, client, ui);

    const view = makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: { lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }) },
      },
    });

    command(view);
    await flushMacrotasks();
    expect(prepareRename).toHaveBeenLastCalledWith('file:///first.ri', 0, 5);

    currentUri = 'file:///second.ri';
    command(view);
    await flushMacrotasks();
    expect(prepareRename).toHaveBeenLastCalledWith('file:///second.ri', 0, 5);
  });

  // -------------------------------------------------------------------------
  // injected applyEdit (multi-file routing) — step-7 tests
  // -------------------------------------------------------------------------

  it('multi-file: injected applyEdit is called with the FULL WorkspaceEdit (not just active uri)', async () => {
    const { client, ui, prepareRename, rename, promptNewName } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    // WorkspaceEdit spans two URIs: the active file AND a sibling.
    const SIBLING_URI = 'file:///other.ri';
    const multiEdit: WorkspaceEdit = {
      changes: {
        [URI]: [{ range: target.range, newText: 'girth' }],
        [SIBLING_URI]: [
          { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } }, newText: 'girth' },
        ],
      },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(multiEdit);

    const applyEdit = vi.fn();
    const view = makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    renameCommand(() => URI, client, ui, applyEdit)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    onSubmit('girth');
    await flushMacrotasks();

    // The injected applyEdit is called with the full multi-uri edit, not just the active URI.
    expect(applyEdit).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledWith(view, multiEdit, URI);
  });

  it('multi-file: stale-apply guard (isConnected=false) prevents the injected applyEdit call', async () => {
    const { client, ui, prepareRename, rename, promptNewName } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    const SIBLING_URI = 'file:///other.ri';
    const multiEdit: WorkspaceEdit = {
      changes: {
        [URI]: [{ range: target.range, newText: 'girth' }],
        [SIBLING_URI]: [
          { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } }, newText: 'girth' },
        ],
      },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(multiEdit);

    const applyEdit = vi.fn();
    // view.dom.isConnected = false → stale guard blocks apply
    const view = makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
      dom: { isConnected: false },
    });

    renameCommand(() => URI, client, ui, applyEdit)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    onSubmit('girth');
    await flushMacrotasks();

    // stale view: rename request fires but injected applyEdit must NOT be called
    expect(rename).toHaveBeenCalledWith(URI, 0, 5, 'girth');
    expect(applyEdit).not.toHaveBeenCalled();
  });

  it('multi-file: file-switch race blocks the injected applyEdit call', async () => {
    const { client, ui, prepareRename, rename, promptNewName } = makeRenameDeps();
    const target = {
      range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
      placeholder: 'width',
    };
    const multiEdit: WorkspaceEdit = {
      changes: {
        [URI]: [{ range: target.range, newText: 'girth' }],
      },
    };
    prepareRename.mockResolvedValue(target);
    rename.mockResolvedValue(multiEdit);

    const applyEdit = vi.fn();
    let currentUri = URI;
    const view = makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    renameCommand(() => currentUri, client, ui, applyEdit)(view);
    await flushMacrotasks();

    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    // Switch files while inline field is open
    currentUri = 'file:///other.ri';
    onSubmit('girth');
    await flushMacrotasks();

    expect(rename).toHaveBeenCalledWith(URI, 0, 5, 'girth');
    expect(applyEdit).not.toHaveBeenCalled();
  });
  // -------------------------------------------------------------------------
  // version-skew guard (injected currentVersion reader) — step-13 tests
  // -------------------------------------------------------------------------

  const GUARD_TARGET = {
    range: { start: { line: 0, character: 5 }, end: { line: 0, character: 10 } },
    placeholder: 'width',
  };

  /** A documentChanges WorkspaceEdit over URI, stamped with `version`. */
  const stampedEdit = (version: number | null): WorkspaceEdit => ({
    documentChanges: [
      {
        textDocument: { uri: URI, version },
        edits: [{ range: GUARD_TARGET.range, newText: 'girth' }],
      },
    ],
  });

  /** The legacy unversioned shape of the same edit. */
  const LEGACY_EDIT: WorkspaceEdit = {
    changes: { [URI]: [{ range: GUARD_TARGET.range, newText: 'girth' }] },
  };

  /** The mock view the guard tests share (head 5 → lspLine 0, lspChar 5). */
  const guardView = () =>
    makeMockView({
      state: {
        selection: { main: { head: 5 } },
        doc: {
          lineAt: (_pos: number) => ({ number: 1, from: 0, to: 20 }),
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

  /** Drive an F2 command through prepareRename → prompt → submit('girth'). */
  async function submitRename(
    command: (view: EditorView) => boolean,
    view: EditorView,
    promptNewName: ReturnType<typeof vi.fn>,
  ): Promise<void> {
    command(view);
    await flushMacrotasks();
    const onSubmit = promptNewName.mock.calls[0][3] as (newName: string) => void;
    onSubmit('girth');
    await flushMacrotasks();
  }

  it('(a) version skew: re-issues the rename ONCE and applies only the fresh edit', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    const fresh = stampedEdit(2);
    // First answer was computed against version 1; the client has since sent 2.
    rename.mockResolvedValueOnce(stampedEdit(1)).mockResolvedValueOnce(fresh);
    currentVersion.mockReturnValue(2);

    const applyEdit = vi.fn();
    const view = guardView();
    await submitRename(
      renameCommand(() => URI, client, ui, applyEdit, currentVersion),
      view,
      promptNewName,
    );

    expect(rename).toHaveBeenCalledTimes(2);
    expect(rename).toHaveBeenNthCalledWith(2, URI, 0, 5, 'girth');
    // ONLY the fresh edit reaches the buffer — the stale one is never applied.
    expect(applyEdit).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledWith(view, fresh, URI);
    expect(showRenameFailed).not.toHaveBeenCalled();
  });

  it('(b) version skew: a second stale edit fails the rename — bounded at one retry, zero edits', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    // The user keeps typing: every answer is stale.
    rename.mockResolvedValue(stampedEdit(1));
    currentVersion.mockReturnValue(2);

    const applyEdit = vi.fn();
    const view = guardView();
    await submitRename(
      renameCommand(() => URI, client, ui, applyEdit, currentVersion),
      view,
      promptNewName,
    );

    // Bounded: exactly two requests, never a third — an unbounded re-issue loop
    // under a typing user would livelock instead of reporting failure.
    expect(rename).toHaveBeenCalledTimes(2);
    expect(applyEdit).not.toHaveBeenCalled();
    expect(showRenameFailed).toHaveBeenCalledTimes(1);
    expect(showRenameFailed).toHaveBeenCalledWith(view);
  });

  it('(c) version skew: an already-fresh edit applies with no retry', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    const fresh = stampedEdit(2);
    rename.mockResolvedValue(fresh);
    currentVersion.mockReturnValue(2);

    const applyEdit = vi.fn();
    const view = guardView();
    await submitRename(
      renameCommand(() => URI, client, ui, applyEdit, currentVersion),
      view,
      promptNewName,
    );

    expect(rename).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledWith(view, fresh, URI);
    expect(showRenameFailed).not.toHaveBeenCalled();
  });

  it('(d) version skew: a legacy unversioned changes edit applies immediately, no retry', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    rename.mockResolvedValue(LEGACY_EDIT);
    // A server that never stamps versions must keep working exactly as before,
    // whatever the client's own counter says.
    currentVersion.mockReturnValue(2);

    const applyEdit = vi.fn();
    const view = guardView();
    await submitRename(
      renameCommand(() => URI, client, ui, applyEdit, currentVersion),
      view,
      promptNewName,
    );

    expect(rename).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledWith(view, LEGACY_EDIT, URI);
    expect(showRenameFailed).not.toHaveBeenCalled();
  });

  it('(e) no currentVersion reader: a stale edit applies exactly as it does today', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    const stale = stampedEdit(1);
    rename.mockResolvedValue(stale);

    const applyEdit = vi.fn();
    const view = guardView();
    // Reader omitted — every existing caller keeps its current behaviour.
    await submitRename(renameCommand(() => URI, client, ui, applyEdit), view, promptNewName);

    expect(rename).toHaveBeenCalledOnce();
    expect(applyEdit).toHaveBeenCalledWith(view, stale, URI);
    expect(showRenameFailed).not.toHaveBeenCalled();
    expect(currentVersion).not.toHaveBeenCalled();
  });

  it('version skew: a file switch during the retry window blocks the fresh apply', async () => {
    const { client, ui, prepareRename, rename, promptNewName, showRenameFailed, currentVersion } =
      makeRenameDeps();
    prepareRename.mockResolvedValue(GUARD_TARGET);
    let currentUri = URI;
    rename
      .mockResolvedValueOnce(stampedEdit(1))
      .mockImplementationOnce(() => {
        // The retry opens a SECOND await window — the user switches files inside it.
        currentUri = 'file:///other.ri';
        return Promise.resolve(stampedEdit(2));
      });
    currentVersion.mockReturnValue(2);

    const applyEdit = vi.fn();
    const view = guardView();
    await submitRename(
      renameCommand(() => currentUri, client, ui, applyEdit, currentVersion),
      view,
      promptNewName,
    );

    expect(rename).toHaveBeenCalledTimes(2);
    // The now-different buffer must not receive the original file's edits.
    expect(applyEdit).not.toHaveBeenCalled();
    expect(showRenameFailed).not.toHaveBeenCalled();
  });
});
