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
import { applyWorkspaceEdit } from '../editor/rename';
import type { WorkspaceEdit } from '../editor/lspClient';

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
});
