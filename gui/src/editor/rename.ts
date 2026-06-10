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
import { lspRangeToCmRange } from './lspRange';

/**
 * Apply an array of LSP TextEdits to a plain string, returning the result.
 *
 * This is the pure core for closed/inactive-file edits: it does NOT need
 * CodeMirror or Tauri — just a source string and LSP TextEdit objects.
 *
 * Algorithm:
 * 1. Build a line-start offset table for the source string.
 * 2. Sort edits by start offset DESCENDING so earlier edits don't shift the
 *    byte positions of later (higher-indexed) edits.
 * 3. Splice each edit (start offset → end offset replaced with newText).
 *
 * Character offsets beyond the line end are clamped to the line end (same
 * semantics as lspPositionToOffset / applyWorkspaceEdit) so malformed server
 * responses don't throw.
 *
 * @param source  The current file content as a string.
 * @param edits   LSP TextEdits with 0-based line/character positions.
 * @returns       The updated string.
 */
export function applyTextEditsToString(
  source: string,
  edits: Array<{
    range: {
      start: { line: number; character: number };
      end: { line: number; character: number };
    };
    newText: string;
  }>,
): string {
  if (edits.length === 0) return source;

  // Build line-start offset table.
  // lineStarts[i] = offset in `source` where line i (0-based) begins.
  const lineStarts: number[] = [0];
  for (let i = 0; i < source.length; i++) {
    if (source[i] === '\n') {
      lineStarts.push(i + 1);
    }
  }

  /** Map (0-based line, 0-based character) → clamped string offset. */
  function posToOffset(line: number, character: number): number {
    const lineStart = lineStarts[line] ?? source.length;
    // Clamp character to the end of the line (next lineStart - 1, or source.length).
    const lineEnd =
      line + 1 < lineStarts.length ? lineStarts[line + 1] - 1 : source.length;
    return Math.min(lineStart + character, lineEnd);
  }

  // Sort descending by start offset so each splice doesn't invalidate later offsets.
  const sorted = [...edits].sort((a, b) => {
    const aOff = posToOffset(a.range.start.line, a.range.start.character);
    const bOff = posToOffset(b.range.start.line, b.range.start.character);
    return bOff - aOff; // descending
  });

  let result = source;
  for (const edit of sorted) {
    const from = posToOffset(edit.range.start.line, edit.range.start.character);
    const to = posToOffset(edit.range.end.line, edit.range.end.character);
    result = result.slice(0, from) + edit.newText + result.slice(to);
  }

  return result;
}

/**
 * Dependency-injected sinks for routing a multi-file WorkspaceEdit.
 *
 * Keeping the sinks as a plain object makes the orchestrator unit-testable
 * without CodeMirror or Tauri — exactly like the existing RenameClient/RenameUi
 * injection pattern.
 */
export interface WorkspaceEditDeps {
  /** Returns true when `uri` is currently open in an editor buffer. */
  isOpen(uri: string): boolean;
  /** Apply edits to the currently active CM view (the single reused EditorView). */
  applyActive(uri: string, edits: WorkspaceEdit['changes'][string]): void;
  /** Apply edits to an open-but-inactive buffer (not the current CM view). */
  applyOpenInactive(uri: string, edits: WorkspaceEdit['changes'][string]): void;
  /** Write edits for a completely closed file directly to disk. */
  applyClosed(uri: string, edits: WorkspaceEdit['changes'][string]): void;
}

/**
 * Route a WorkspaceEdit's per-URI edits to the appropriate sink.
 *
 * Routing logic per URI:
 *  - uri === activeUri         → deps.applyActive (uses the live CM view)
 *  - deps.isOpen(uri) === true → deps.applyOpenInactive (buffer update + persist)
 *  - else                      → deps.applyClosed (direct disk write)
 *
 * URIs with an absent or empty edit list are silently skipped.
 * Pure routing — no I/O of its own.
 */
export function applyWorkspaceEditAcrossFiles(
  edit: WorkspaceEdit,
  activeUri: string,
  deps: WorkspaceEditDeps,
): void {
  const changes = edit.changes;
  if (!changes) return;

  for (const [uri, edits] of Object.entries(changes)) {
    if (!edits || edits.length === 0) continue;

    if (uri === activeUri) {
      deps.applyActive(uri, edits);
    } else if (deps.isOpen(uri)) {
      deps.applyOpenInactive(uri, edits);
    } else {
      deps.applyClosed(uri, edits);
    }
  }
}

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
  const changes = edits.flatMap((e) => {
    const r = lspRangeToCmRange(doc, e.range);
    if (!r) return []; // out-of-range (version skew) — skip without throwing
    return [{ from: r.from, to: r.to, insert: e.newText }];
  });

  if (changes.length === 0) return false;
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
