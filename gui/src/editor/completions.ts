/**
 * CodeMirror completion source powered by the in-process LSP server.
 *
 * Translates CodeMirror's CompletionContext into LSP textDocument/completion
 * requests and maps the returned CompletionItems back to CodeMirror Completion
 * objects.
 */
import type {
  CompletionContext,
  CompletionResult,
  CompletionSource,
  Completion,
} from '@codemirror/autocomplete';
import { invoke } from '@tauri-apps/api/core';

// LSP CompletionItemKind → CodeMirror completion type mapping
const LSP_KIND_MAP: Record<number, string> = {
  1: 'text',        // Text
  2: 'method',      // Method
  3: 'function',    // Function
  4: 'method',      // Constructor
  5: 'variable',    // Field
  6: 'variable',    // Variable
  7: 'class',       // Class
  8: 'interface',   // Interface
  9: 'namespace',   // Module
  10: 'property',   // Property
  13: 'enum',       // Enum
  14: 'keyword',    // Keyword
  15: 'text',       // Snippet
  21: 'constant',   // Constant
  22: 'class',      // Struct
  25: 'type',       // TypeParameter
};

function lspKindToType(kind?: number): string | undefined {
  return kind ? LSP_KIND_MAP[kind] : undefined;
}

interface LspCompletionItem {
  label: string;
  kind?: number;
  detail?: string;
  insertText?: string;
  documentation?: string | { kind: string; value: string };
}

/**
 * Create a CodeMirror CompletionSource for the given document URI.
 *
 * Accepts either a static URI string or a `() => string` getter for dynamic
 * URI resolution. When a getter is provided, the URI is resolved on each
 * completion request, ensuring the correct file is targeted after file switches.
 *
 * Returns a function that, when called with a CompletionContext, sends a
 * textDocument/completion request to the LSP server and converts the
 * response to CodeMirror's CompletionResult format.
 */
export function reifyCompletionSource(uri: string | (() => string)): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const { pos, state } = context;
    const line = state.doc.lineAt(pos);
    const lspLine = line.number - 1; // CodeMirror is 1-based, LSP is 0-based
    const lspChar = pos - line.from;
    const resolvedUri = typeof uri === 'function' ? uri() : uri;

    const params = JSON.stringify({
      textDocument: { uri: resolvedUri },
      position: { line: lspLine, character: lspChar },
    });

    try {
      const response = await invoke<string>('lsp_request', {
        method: 'textDocument/completion',
        params,
      });

      const parsed = JSON.parse(response);
      let items: LspCompletionItem[];
      if (Array.isArray(parsed)) {
        items = parsed;
      } else if (parsed && Array.isArray(parsed.items)) {
        items = parsed.items;
      } else {
        return null;
      }

      if (items.length === 0) {
        return null;
      }

      const options: Completion[] = items.map((item) => {
        const completion: Completion = {
          label: item.label,
          type: lspKindToType(item.kind),
        };
        if (item.detail) {
          completion.detail = item.detail;
        }
        if (item.insertText) {
          completion.apply = item.insertText;
        }
        if (item.documentation) {
          completion.info =
            typeof item.documentation === 'string'
              ? item.documentation
              : item.documentation.value;
        }
        return completion;
      });

      // Scan backward from cursor to find the start of the current word.
      // This ensures that accepting a completion replaces the partial word
      // rather than inserting the full label at the cursor position.
      const lineText = state.doc.sliceString(line.from, pos);
      let wordStart = pos;
      for (let i = lineText.length - 1; i >= 0; i--) {
        if (/[a-zA-Z0-9_]/.test(lineText[i])) {
          wordStart = line.from + i;
        } else {
          break;
        }
      }

      return {
        from: wordStart,
        options,
      };
    } catch {
      return null;
    }
  };
}
