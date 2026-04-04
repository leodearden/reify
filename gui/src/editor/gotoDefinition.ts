/**
 * CodeMirror Ctrl+Click go-to-definition handler powered by the LSP server.
 *
 * Provides a keymap/event handler that on Ctrl+Click (or Cmd+Click on Mac)
 * sends a textDocument/definition request and navigates to the result.
 */
import { type Extension } from '@codemirror/state';
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

      // Fire async, don't block the event
      requestDefinition(currentUri, lspLine, lspChar).then((location) => {
        if (!location) return;
        if (!view.dom.isConnected) return;

        const resolvedNow = resolveUri(uri);
        const sameFile = isSameFile(location.uri, resolvedNow);

        if (sameFile) {
          // Same document: navigate to definition in current view
          const targetLine = view.state.doc.line(location.range.start.line + 1);
          const targetPos = targetLine.from + location.range.start.character;
          view.dispatch({
            selection: { anchor: targetPos },
            scrollIntoView: true,
          });
        } else if (onNavigate) {
          // Different file: delegate to the onNavigate callback
          onNavigate(
            location.uri,
            location.range.start.line,
            location.range.start.character,
          );
        }
      });

      return true; // Consume the event
    },
  });
}
