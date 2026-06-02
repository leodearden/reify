/**
 * CodeMirror Ctrl+Click go-to-definition handler powered by the LSP server.
 *
 * Provides a keymap/event handler that on Ctrl+Click (or Cmd+Click on Mac)
 * sends a textDocument/definition request and navigates to the result.
 *
 * Also exports gotoDefinitionCommand — a CodeMirror Command factory that reads
 * the cursor from view.state.selection.main.head and runs the same logic,
 * suitable for use with keymap.of in Editor.tsx.
 */
import { type Extension, type Text, type Line } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';
import { isSameFile } from '../utils/pathUtils';
import type { NavEntry } from '../hooks/useNavHistory';

interface LspLocation {
  uri: string;
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

/**
 * Resolve a URI from either a static string or getter.
 */
function resolveUri(uri: string | (() => string)): string {
  return typeof uri === 'function' ? uri() : uri;
}

/**
 * Send a textDocument/definition request for the given position.
 */
async function requestDefinition(
  uri: string,
  line: number,
  character: number,
): Promise<LspLocation | null> {
  const params = JSON.stringify({
    textDocument: { uri },
    position: { line, character },
  });

  try {
    const response = await invoke<string>('lsp_request', {
      method: 'textDocument/definition',
      params,
    });

    const parsed = JSON.parse(response);
    if (!parsed) return null;

    // Handle both Location and LocationLink formats
    if (parsed.uri && parsed.range) {
      return parsed as LspLocation;
    }
    if (parsed.targetUri && parsed.targetRange) {
      return {
        uri: parsed.targetUri,
        range: parsed.targetRange,
      };
    }
    // Array of locations — take the first
    if (Array.isArray(parsed) && parsed.length > 0) {
      const first = parsed[0];
      if (first.uri && first.range) return first;
      if (first.targetUri && first.targetRange) {
        return { uri: first.targetUri, range: first.targetRange };
      }
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Return true if `value` is a non-negative integer — the minimum shape
 * required for any LSP position component (line or character).
 *
 * Rejects NaN, null, Infinity, fractional numbers, and negative values in one
 * call. Used as a fast pre-filter before any document-aware bound check.
 */
function isValidPositionShape(value: number): boolean {
  return Number.isInteger(value) && value >= 0;
}

/**
 * Validate an LSP (0-based) position against the current document.
 *
 * Returns the resolved CodeMirror `Line` on success, or `null` if the
 * position is invalid (negative, beyond document bounds, or character past
 * end-of-line). Groups all same-file invariants in one place so the
 * validation surface is easy to audit.
 */
function isValidLspPosition(
  doc: Text,
  line: number,
  character: number,
): { targetLine: Line } | null {
  if (!isValidPositionShape(line) || !isValidPositionShape(character)) return null;
  if (line + 1 > doc.lines) return null;
  const targetLine = doc.line(line + 1);
  if (character > targetLine.to - targetLine.from) return null;
  return { targetLine };
}

/**
 * Shared resolve-then-navigate core used by both the mousedown handler and
 * gotoDefinitionCommand.
 *
 * @param currentUri    The URI of the document being navigated from.
 * @param lspLine       0-based LSP line number of the cursor position.
 * @param lspChar       0-based LSP character offset of the cursor position.
 * @param view          The CodeMirror EditorView.
 * @param uriGetter     Getter for the URI at call time (re-resolved after async).
 * @param onNavigate    Called for cross-file results.
 * @param onRecordJump  Optional hook called on successful same-file navigation
 *                      with (origin, dest).  origin is captured synchronously
 *                      before the async request; dest is computed on success.
 * @param originOffset  CodeMirror offset of the originating cursor position,
 *                      used to build the origin NavEntry for onRecordJump.
 */
function resolveAndNavigate(
  currentUri: string,
  lspLine: number,
  lspChar: number,
  view: EditorView,
  uriGetter: string | (() => string),
  onNavigate?: (targetUri: string, line: number, character: number) => void,
  onRecordJump?: (origin: NavEntry, dest: NavEntry) => void,
  originOffset?: number,
): void {
  // Capture origin synchronously before the async request, only when needed.
  const capturedOriginUri = currentUri;
  const capturedOriginOffset = originOffset;

  requestDefinition(currentUri, lspLine, lspChar).then((location) => {
    if (!location) return;
    if (!view.dom.isConnected) return;

    const resolvedNow = resolveUri(uriGetter);
    const sameFile = isSameFile(location.uri, resolvedNow);

    if (sameFile) {
      // Same document: navigate to definition in current view.
      const valid = isValidLspPosition(
        view.state.doc,
        location.range.start.line,
        location.range.start.character,
      );
      if (!valid) return;
      const targetPos = valid.targetLine.from + location.range.start.character;
      view.dispatch({
        selection: { anchor: targetPos },
        scrollIntoView: true,
      });
      // Record jump for nav history if both hook and origin are provided.
      if (onRecordJump && capturedOriginOffset !== undefined) {
        onRecordJump(
          { uri: capturedOriginUri, offset: capturedOriginOffset },
          { uri: resolvedNow, offset: targetPos },
        );
      }
    } else if (onNavigate) {
      // Different file: delegate to the onNavigate callback.
      // Minimum guard: reject non-integer/negative positions before delegating.
      // Full doc-aware validation (character vs. line length) happens in the
      // consumer (Editor.tsx) against the target file once it is opened.
      if (
        !isValidPositionShape(location.range.start.line) ||
        !isValidPositionShape(location.range.start.character)
      ) return;
      onNavigate(
        location.uri,
        location.range.start.line,
        location.range.start.character,
      );
    }
  }).catch((err) => console.warn('gotoDefinition: failed to apply result', err));
}

/**
 * Create a CodeMirror extension that handles Ctrl+Click for go-to-definition.
 *
 * Accepts either a static URI string or a `() => string` getter for dynamic
 * URI resolution after file switches.
 *
 * When the user Ctrl+clicks (or Cmd+clicks) on a symbol, sends a
 * textDocument/definition request and moves the cursor to the result
 * if it's in the same document.
 *
 * @param uri - The document URI or getter to use for LSP requests.
 * @param onNavigate - Called for cross-file navigation results.
 * @param onRecordJump - Optional hook for recording same-file jumps in nav history.
 */
export function reifyGotoDefinition(
  uri: string | (() => string),
  onNavigate?: (targetUri: string, line: number, character: number) => void,
  onRecordJump?: (origin: NavEntry, dest: NavEntry) => void,
): Extension {
  return EditorView.domEventHandlers({
    mousedown(event: MouseEvent, view: EditorView) {
      // Only handle Ctrl+Click (or Cmd+Click on Mac)
      if (!(event.ctrlKey || event.metaKey)) return false;

      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;

      const line = view.state.doc.lineAt(pos);
      const lspLine = line.number - 1;
      const lspChar = pos - line.from;
      const currentUri = resolveUri(uri);

      // Unlike hoverTooltip/completions, this promise is detached from
      // CodeMirror's lifecycle — guard against dispatch after view destruction.
      resolveAndNavigate(currentUri, lspLine, lspChar, view, uri, onNavigate, onRecordJump, pos);

      return true; // Consume the event
    },
  });
}

/**
 * Create a CodeMirror Command for F12 go-to-definition.
 *
 * Returns a `(view: EditorView) => boolean` function suitable for use in
 * keymap.of. Reads the cursor position from view.state.selection.main.head
 * and runs the same resolve-then-navigate logic as reifyGotoDefinition.
 *
 * No @codemirror/view or @codemirror/state runtime imports are added here;
 * the keymap.of wrapping remains in Editor.tsx.
 *
 * @param uriGetter  Getter for the current document URI.
 * @param onNavigate Called for cross-file navigation results.
 * @param onRecordJump Optional hook for recording same-file jumps in nav history.
 */
export function gotoDefinitionCommand(
  uriGetter: () => string,
  onNavigate?: (targetUri: string, line: number, character: number) => void,
  onRecordJump?: (origin: NavEntry, dest: NavEntry) => void,
): (view: EditorView) => boolean {
  return (view: EditorView): boolean => {
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head);
    const lspLine = line.number - 1;
    const lspChar = head - line.from;
    const currentUri = uriGetter();

    resolveAndNavigate(currentUri, lspLine, lspChar, view, uriGetter, onNavigate, onRecordJump, head);

    return true; // Always consume the key
  };
}
