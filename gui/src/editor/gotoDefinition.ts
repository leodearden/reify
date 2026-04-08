/**
 * CodeMirror Ctrl+Click go-to-definition handler powered by the LSP server.
 *
 * Provides a keymap/event handler that on Ctrl+Click (or Cmd+Click on Mac)
 * sends a textDocument/definition request and navigates to the result.
 */
import { type Extension, type Text, type Line } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';
import { isSameFile } from '../utils/pathUtils';

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
  if (!Number.isInteger(line) || !Number.isInteger(character)) return null;
  if (line < 0 || line + 1 > doc.lines) return null;
  const targetLine = doc.line(line + 1);
  if (character < 0 || character > targetLine.to - targetLine.from) return null;
  return { targetLine };
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
 */
export function reifyGotoDefinition(
  uri: string | (() => string),
  onNavigate?: (targetUri: string, line: number, character: number) => void,
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
      requestDefinition(currentUri, lspLine, lspChar).then((location) => {
        if (!location) return;
        if (!view.dom.isConnected) return;

        const resolvedNow = resolveUri(uri);
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
        } else if (onNavigate) {
          // Different file: delegate to the onNavigate callback.
          // Minimum guard: reject negative positions before delegating. Full
          // doc-aware validation (character vs. line length) happens in the
          // consumer (Editor.tsx) against the target file once it is opened.
          if (location.range.start.line < 0 || location.range.start.character < 0) return;
          onNavigate(
            location.uri,
            location.range.start.line,
            location.range.start.character,
          );
        }
      }).catch((err) => console.warn('gotoDefinition: failed to apply result', err));

      return true; // Consume the event
    },
  });
}
