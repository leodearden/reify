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
import { applyWorkspaceEdit, renameCommand } from '../editor/rename';
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
});

// ---------------------------------------------------------------------------
// renameCommand — CodeMirror Command factory (cursor → prepareRename → UI)
// ---------------------------------------------------------------------------

/** Build a mocked rename client + ui from individually-trackable vi.fns. */
function makeRenameDeps() {
  const prepareRename = vi.fn();
  const rename = vi.fn();
  const promptNewName = vi.fn();
  const showCannotRename = vi.fn();
  const showRenameFailed = vi.fn();
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
});
