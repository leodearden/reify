/**
 * F2 inline-rename editor glue, powered by the LSP prepareRename / rename
 * requests.
 *
 * Two pieces, both pure and unit-testable (see rename.test.ts):
 *
 *  - applyWorkspaceEdit(view, edit, uri): convert an LSP WorkspaceEdit's per-URI
 *    TextEdits into a SINGLE CodeMirror view.dispatch (multi-change). Routing the
 *    rename through one dispatch lets it flow through Editor.tsx's existing
 *    updateListener → markDirty + debounced updateSource + LSP didChange, exactly
 *    like a hand edit — no separate backend-sync path.
 *
 *  - renameCommand(uriGetter, client, ui): a CodeMirror Command factory (added in
 *    a later step) that reads the cursor, calls prepareRename, and either refuses
 *    or opens the inline field via injected UI callbacks.
 */
import type { EditorView } from '@codemirror/view';
import type { WorkspaceEdit } from './lspClient';

/**
 * Apply an LSP WorkspaceEdit's edits for `uri` to the editor as one transaction.
 *
 * Each LSP TextEdit range (0-based line/character) is mapped to CodeMirror
 * document offsets via `doc.line(line + 1).from + character`, clamped to the line
 * end as defense-in-depth (compute_rename already emits ascending, in-line,
 * non-overlapping name-token edits). All edits are dispatched together so the
 * rename is a single atomic, undo-able operation.
 *
 * Returns false WITHOUT dispatching when the edit carries no changes for `uri`
 * (absent `changes` map, missing key, or empty list) — the caller can treat that
 * as "nothing to apply".
 */
export function applyWorkspaceEdit(
  view: EditorView,
  edit: WorkspaceEdit,
  uri: string,
): boolean {
  const edits = edit.changes?.[uri];
  if (!edits || edits.length === 0) return false;

  const doc = view.state.doc;
  const changes = edits.map((e) => {
    const startLine = doc.line(e.range.start.line + 1);
    const endLine = doc.line(e.range.end.line + 1);
    const from = Math.min(startLine.from + e.range.start.character, startLine.to);
    const to = Math.min(endLine.from + e.range.end.character, endLine.to);
    return { from, to, insert: e.newText };
  });

  view.dispatch({ changes, userEvent: 'rename' });
  return true;
}
