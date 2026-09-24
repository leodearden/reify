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
import type { PrepareRenameResult, Range, TextEdit, WorkspaceEdit } from './lspClient';
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
 * One document's worth of a WorkspaceEdit, flattened out of whichever wire
 * representation the server used.
 *
 * `version` is the document version the server computed these edits against, or
 * `null` when the server did not state one (the legacy `changes` map, or a file
 * not open on the server whose content on disk is master).
 */
interface WorkspaceEditTarget {
  uri: string;
  version: number | null;
  edits: TextEdit[];
}

/**
 * Read a WorkspaceEdit's per-document edits out of either wire representation.
 *
 * The SINGLE reader of that wire shape: `documentChanges` takes precedence over
 * `changes` per the LSP spec, and every consumer — both appliers and the
 * staleness check — goes through here so the precedence rule cannot drift
 * between them.
 *
 * "No version stated" has two spellings on the wire — reify's server always
 * sends an explicit `null`, but the spec lets a server omit the key — and both
 * collapse to `null` HERE, so every consumer downstream has exactly one absent
 * form to test against.
 */
function workspaceEditTargets(edit: WorkspaceEdit): WorkspaceEditTarget[] {
  if (edit.documentChanges) {
    return edit.documentChanges.map((entry) => ({
      uri: entry.textDocument.uri,
      version: entry.textDocument.version ?? null,
      edits: entry.edits,
    }));
  }
  return Object.entries(edit.changes ?? {}).map(([uri, edits]) => ({
    uri,
    version: null,
    edits,
  }));
}

/**
 * Reads the version the client last SENT to the server for `uri`.
 *
 * `undefined` means the client never tracked that URI — not that it is at
 * version zero.
 */
export type DocumentVersionReader = (uri: string) => number | undefined;

/**
 * URIs whose edits were computed against a document version the client no
 * longer holds.
 *
 * A URI is stale only on DEMONSTRATED disagreement — both versions known and
 * different. Every other combination is not-stale by construction:
 *
 *  - `version === null` — the server stated no version, either explicitly or by
 *    omitting the key (both normalise to `null` in workspaceEditTargets). It
 *    does not have this document open, so the content on disk is master and
 *    there is no version to compare against. Refusing
 *    here would reject every legitimate cross-file rename touching a closed
 *    file, and it is also the whole of the legacy unversioned `changes` shape.
 *  - client version `undefined` — the client never tracked this URI, so
 *    staleness is unknowable rather than proven. Refusing here would break the
 *    closed-file and inactive-buffer sinks.
 *
 * This detects exactly the race it is named for: the server computed these
 * edits against version N, and the client has since sent M. It cannot see
 * local edits not yet sent, so a caller must make sure the server has the
 * latest text before asking (Editor.tsx flushes the pending didChange before
 * F2); `lspRangeToCmRange`'s null-skip remains a last-resort backstop.
 */
function staleEditTargets(
  edit: WorkspaceEdit,
  currentVersion: DocumentVersionReader,
): string[] {
  return workspaceEditTargets(edit)
    .filter(({ uri, version }) => {
      if (version === null) return false;
      const held = currentVersion(uri);
      return held !== undefined && held !== version;
    })
    .map(({ uri }) => uri);
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
  applyActive(uri: string, edits: TextEdit[]): void;
  /** Apply edits to an open-but-inactive buffer (not the current CM view). */
  applyOpenInactive(uri: string, edits: TextEdit[]): void;
  /** Write edits for a completely closed file directly to disk. */
  applyClosed(uri: string, edits: TextEdit[]): void;
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
  for (const { uri, edits } of workspaceEditTargets(edit)) {
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
 * Returns false WITHOUT dispatching when the edit carries no edits for `uri`
 * (neither representation present, no target for that URI, or an empty list) —
 * the caller can treat that as "nothing to apply".
 */
export function applyWorkspaceEdit(
  view: EditorView,
  edit: WorkspaceEdit,
  uri: string,
): boolean {
  const edits = workspaceEditTargets(edit).find((t) => t.uri === uri)?.edits;
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
 * Callback type for the injected multi-file edit applicator.
 *
 * `view`      — the live CodeMirror EditorView (for the active file).
 * `edit`      — the FULL WorkspaceEdit, potentially spanning multiple URIs.
 * `activeUri` — the URI that was active when the rename was initiated.
 *
 * The callback is responsible for routing each URI's edits to the appropriate
 * sink (active CM view, inactive open buffer, or disk write).  If omitted,
 * renameCommand falls back to the single-uri `applyWorkspaceEdit` (original
 * behaviour, preserves backward-compat for callers that don't need cross-file
 * routing).
 */
export type ApplyEditFn = (view: EditorView, edit: WorkspaceEdit, activeUri: string) => void;

/**
 * Request a rename edit that is safe to apply, re-issuing ONCE on version skew.
 *
 * An edit whose stamped versions disagree with the client's describes a document
 * the client no longer holds, so its ranges no longer point at the text they
 * were computed from. Asking again is the only recovery available from here: the
 * second request is answered against the version the client has since sent.
 *
 * The re-issue is bounded at one. A user typing through the debounced didChange
 * can invalidate every answer in turn, and an unbounded loop would spin against
 * them instead of reporting that the rename did not apply.
 *
 * Returns null when the server refused the name outright, or when the re-issued
 * edit was stale too — both mean "apply nothing, tell the user".
 *
 * With no `currentVersion` reader nothing is ever judged stale, so the single
 * request and its answer pass straight through.
 */
async function resolveApplicableEdit(
  client: RenameClient,
  uri: string,
  line: number,
  character: number,
  newName: string,
  currentVersion?: DocumentVersionReader,
): Promise<WorkspaceEdit | null> {
  const applicable = (edit: WorkspaceEdit | null): boolean =>
    !!edit && (!currentVersion || staleEditTargets(edit, currentVersion).length === 0);

  const first = await client.rename(uri, line, character, newName);
  if (!first || applicable(first)) return first ?? null;

  const reissued = await client.rename(uri, line, character, newName);
  return applicable(reissued) ? reissued : null;
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
 *    and its WorkspaceEdit applied via the injected `applyEdit` callback (which
 *    routes the edit across ALL changed files). When `applyEdit` is omitted the
 *    original single-uri `applyWorkspaceEdit` is used as the fallback.
 *
 * Always returns true so the F2 key is consumed. Both the prompt-open and
 * apply steps re-check that the URI is still current (and the apply step also
 * checks view.dom.isConnected) so stale applies never corrupt the active buffer.
 *
 * `currentVersion`, when supplied, arms the version-skew guard: an edit computed
 * against a document version the client has already moved past is re-requested
 * rather than applied (see resolveApplicableEdit). Omitting it leaves the
 * unguarded behaviour untouched.
 */
export function renameCommand(
  uriGetter: () => string,
  client: RenameClient,
  ui: RenameUi,
  applyEdit?: ApplyEditFn,
  currentVersion?: DocumentVersionReader,
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
            resolveApplicableEdit(client, uri, lspLine, lspChar, newName, currentVersion)
              .then((edit) => {
                // The field can outlive the editor, and the user may switch files
                // while the rename is in flight — never mutate a dead or
                // now-different view; a stale apply would corrupt the new file.
                // Checked HERE, after the LAST await, so a re-issued request's
                // second await window is covered by the same guard.
                if (!view.dom.isConnected || uriGetter() !== uri) return;
                if (!edit) {
                  // Server rejected the accepted name (invalid identifier /
                  // no-op), or every answer it gave was stale: the field already
                  // closed, so surface a transient message instead of dropping
                  // the rename silently.
                  ui.showRenameFailed(view);
                  return;
                }
                if (applyEdit) {
                  // Injected multi-file applicator: routes each URI's edits to
                  // the right sink (active CM view / open buffer / disk).
                  applyEdit(view, edit, uri);
                } else {
                  // Fallback: single-uri apply (backward-compat for tests/callers
                  // that don't supply the cross-file routing callback).
                  applyWorkspaceEdit(view, edit, uri);
                }
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
