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
import type { PrepareRenameResult, Range, WorkspaceEdit } from './lspClient';

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

/**
 * The LSP surface renameCommand depends on — a structural subset of LspClient.
 * Injecting it (rather than importing the singleton) keeps the refuse/accept
 * routing unit-testable with mocked prepareRename/rename.
 */
export interface RenameClient {
  prepareRename(
    uri: string,
    line: number,
    character: number,
  ): Promise<PrepareRenameResult | null>;
  rename(
    uri: string,
    line: number,
    character: number,
    newName: string,
  ): Promise<WorkspaceEdit | null>;
}

/**
 * Editor-supplied UI callbacks for the inline rename flow.
 *
 * Injected (rather than hard-wired DOM) so the command's routing can be tested
 * without CodeMirror layout — the inline-field DOM lives in Editor.tsx.
 */
export interface RenameUi {
  /**
   * Open the inline rename field over `range`, pre-filled with `placeholder`.
   * `onSubmit(newName)` runs the rename; `onCancel()` dismisses with no edit.
   */
  promptNewName(
    view: EditorView,
    range: Range,
    placeholder: string,
    onSubmit: (newName: string) => void,
    onCancel: () => void,
  ): void;
  /** Show a transient "can't rename here" message (the Invariant-4 refusal). */
  showCannotRename(view: EditorView): void;
  /**
   * Show a transient "rename failed" message when the server rejects an accepted
   * new name (invalid identifier / no-op) and returns no edit. The inline field
   * has already closed by then, so this is the only feedback the user gets that
   * their rename did not apply.
   */
  showRenameFailed(view: EditorView): void;
}

/**
 * Create a CodeMirror Command for F2 rename.
 *
 * Returns a `(view) => boolean` suitable for keymap.of in Editor.tsx. It reads
 * the cursor from view.state.selection.main.head, derives the 0-based LSP
 * line/character, and calls prepareRename:
 *
 *  - null target → ui.showCannotRename(view): the hard safety guard. A
 *    non-renameable position (keyword/literal/builtin/type/decl/cross-module)
 *    performs ZERO edits — only a transient message.
 *  - non-null target → ui.promptNewName(...). On submit, rename() is requested
 *    and its WorkspaceEdit applied via applyWorkspaceEdit (one CM dispatch that
 *    flows through Editor.tsx's updateListener → backend sync + didChange).
 *
 * Always returns true so the F2 key is consumed. The async request can outlive
 * the editor or a file switch (the EditorView is reused across files — its doc is
 * swapped in place), so both the prompt-open and apply steps re-check that the URI
 * is still current (and the apply step also checks view.dom.isConnected) before
 * touching the buffer; a stale apply would corrupt the newly-active file. A
 * server-rejected name (rename → null) closes the field and surfaces a transient
 * ui.showRenameFailed message rather than dropping the rename silently.
 */
export function renameCommand(
  uriGetter: () => string,
  client: RenameClient,
  ui: RenameUi,
): (view: EditorView) => boolean {
  return (view: EditorView): boolean => {
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head);
    const lspLine = line.number - 1;
    const lspChar = head - line.from;
    const uri = uriGetter();

    client
      .prepareRename(uri, lspLine, lspChar)
      .then((target) => {
        // A file switch can swap the buffer while prepareRename is in flight (the
        // editor reuses one EditorView across files). Abandon a stale request
        // rather than prompting/refusing on the now-different document.
        if (uriGetter() !== uri) return;
        if (!target) {
          // Invariant-4 refusal: show the message, edit nothing.
          ui.showCannotRename(view);
          return;
        }
        ui.promptNewName(
          view,
          target.range,
          target.placeholder,
          (newName: string) => {
            client
              .rename(uri, lspLine, lspChar, newName)
              .then((edit) => {
                // The field can outlive the editor, and the user may switch files
                // while the rename is in flight — never mutate a dead or
                // now-different view; a stale apply would corrupt the new file.
                if (!view.dom.isConnected || uriGetter() !== uri) return;
                if (!edit) {
                  // Server rejected the accepted name (invalid identifier /
                  // no-op): the field already closed, so surface a transient
                  // message instead of dropping the rename silently.
                  ui.showRenameFailed(view);
                  return;
                }
                applyWorkspaceEdit(view, edit, uri);
              })
              .catch((err) => console.warn('rename: failed to apply edit', err));
          },
          () => {
            // onCancel: the field dismisses itself; nothing to undo here.
          },
        );
      })
      .catch((err) => console.warn('rename: prepareRename failed', err));

    return true; // Always consume the key.
  };
}
