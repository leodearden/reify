/**
 * CodeMirror hover tooltip powered by the in-process LSP server.
 *
 * Provides a hoverTooltip extension that calls textDocument/hover
 * on the LSP server and displays the result as a tooltip.
 */
import { hoverTooltip, type Tooltip } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';

interface LspHoverResult {
  contents:
    | string
    | { kind: string; value: string }
    | Array<string | { language: string; value: string }>;
  range?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

/**
 * Extract plain text from LSP hover contents.
 */
function extractHoverText(contents: LspHoverResult['contents']): string {
  if (typeof contents === 'string') return contents;
  if (Array.isArray(contents)) {
    return contents
      .map((c) => (typeof c === 'string' ? c : c.value))
      .join('\n');
  }
  if (contents && typeof contents === 'object' && 'value' in contents) {
    return (contents as { value: string }).value;
  }
  return '';
}

/**
 * Create a CodeMirror hoverTooltip extension for LSP hover.
 *
 * Accepts either a static URI string or a `() => string` getter for dynamic
 * URI resolution after file switches.
 *
 * @param uri - The document URI or getter to use for LSP requests.
 */
export function reifyHoverTooltip(uri: string | (() => string)) {
  return hoverTooltip(async (_view, pos, _side): Promise<Tooltip | null> => {
    const state = _view.state;
    const line = state.doc.lineAt(pos);
    const lspLine = line.number - 1;
    const lspChar = pos - line.from;
    const resolvedUri = typeof uri === 'function' ? uri() : uri;

    const params = JSON.stringify({
      textDocument: { uri: resolvedUri },
      position: { line: lspLine, character: lspChar },
    });

    try {
      const response = await invoke<string>('lsp_request', {
        method: 'textDocument/hover',
        params,
      });

      const parsed = JSON.parse(response) as LspHoverResult | null;
      if (!parsed || !parsed.contents) return null;

      const text = extractHoverText(parsed.contents);
      if (!text) return null;

      return {
        pos,
        above: true,
        create() {
          const dom = document.createElement('div');
          dom.className = 'cm-lsp-hover';
          dom.style.padding = '4px 8px';
          dom.style.maxWidth = '400px';
          dom.style.whiteSpace = 'pre-wrap';
          dom.textContent = text;
          return { dom };
        },
      };
    } catch {
      return null;
    }
  });
}
