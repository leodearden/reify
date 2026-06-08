/**
 * LSP client wrapper for the in-process LSP server.
 *
 * Provides typed methods for LSP requests and notifications, calling through
 * the Tauri `lsp_request` bridge command. The backend deserializes params JSON,
 * dispatches to the in-process LanguageServer, and returns the JSON response.
 */
import { invoke } from '@tauri-apps/api/core';

// ── Types ──────────────────────────────────────────────────────────────

export interface Range {
  start: { line: number; character: number };
  end: { line: number; character: number };
}

export interface DocumentSymbol {
  name: string;
  detail?: string;
  kind: number;
  range: Range;
  selectionRange: Range;
  children?: DocumentSymbol[];
}

/**
 * A single occurrence highlight (LSP `DocumentHighlight`). `kind` is the
 * read/write classification (1 = Text); Reify's δ producer always emits Text.
 */
export interface DocumentHighlight {
  range: Range;
  kind?: number;
}

export interface ServerCapabilities {
  completionProvider?: unknown;
  hoverProvider?: boolean | unknown;
  definitionProvider?: boolean | unknown;
  documentSymbolProvider?: boolean | unknown;
  textDocumentSync?: unknown;
}

export interface InitializeResult {
  capabilities: ServerCapabilities;
}

export interface CompletionItem {
  label: string;
  kind?: number;
  detail?: string;
  insertText?: string;
  documentation?: string | { kind: string; value: string };
}

export interface HoverResult {
  contents: string | { kind: string; value: string } | unknown;
  range?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

export interface Location {
  uri: string;
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

/** A single text edit: replace `range` with `newText`. */
export interface TextEdit {
  range: Range;
  newText: string;
}

/** An LSP WorkspaceEdit. Reify's rename only ever populates `changes`. */
export interface WorkspaceEdit {
  changes?: { [uri: string]: TextEdit[] };
}

/** Result of a successful prepareRename: the token range + its current name. */
export interface PrepareRenameResult {
  range: Range;
  placeholder: string;
}

// ── LSP Client interface ───────────────────────────────────────────────

export interface LspClient {
  initialize(): Promise<InitializeResult>;
  initialized(): Promise<void>;
  didOpen(uri: string, text: string, version: number): Promise<void>;
  didChange(uri: string, text: string, version: number): Promise<void>;
  didClose(uri: string): Promise<void>;
  completion(uri: string, line: number, character: number): Promise<CompletionItem[]>;
  hover(uri: string, line: number, character: number): Promise<HoverResult | null>;
  gotoDefinition(uri: string, line: number, character: number): Promise<Location | null>;
  documentSymbol(uri: string): Promise<DocumentSymbol[]>;
  documentHighlight(
    uri: string,
    line: number,
    character: number,
  ): Promise<DocumentHighlight[]>;
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
  references(
    uri: string,
    line: number,
    character: number,
    includeDeclaration: boolean,
  ): Promise<Location[]>;
}

// ── Shared helpers ─────────────────────────────────────────────────────

/**
 * Extract plain markdown text from LSP hover `contents`.
 *
 * Handles the three shapes the LSP spec allows:
 * - `string` — returned as-is
 * - `{ kind, value }` (MarkupContent) — returns `value`
 * - `Array<string | { language, value }>` — joins extracted values with `\n`
 * - anything else (null, undefined, unknown object) — returns `''`
 *
 * Extracted from the private `extractHoverText` in hover.ts so that
 * bridge.ts's hover_at probe and hover.ts's tooltip cannot diverge.
 */
export function extractHoverMarkdown(
  contents: unknown,
): string {
  if (contents === null || contents === undefined) return '';
  if (typeof contents === 'string') return contents;
  if (Array.isArray(contents)) {
    return (contents as Array<unknown>)
      .map((c) =>
        typeof c === 'string'
          ? c
          : c !== null && typeof c === 'object' && 'value' in c
            ? (c as { value: string }).value
            : '',
      )
      .filter(Boolean)
      .join('\n');
  }
  if (typeof contents === 'object' && 'value' in (contents as object)) {
    return (contents as { value: string }).value;
  }
  return '';
}

async function lspRequest(method: string, params: unknown): Promise<string> {
  return invoke<string>('lsp_request', {
    method,
    params: JSON.stringify(params),
  });
}

// ── Factory ────────────────────────────────────────────────────────────

/**
 * Create an LSP client instance that communicates with the in-process
 * LSP server through the Tauri bridge.
 */
export function createLspClient(): LspClient {
  return {
    async initialize(): Promise<InitializeResult> {
      const response = await lspRequest('initialize', { capabilities: {} });
      return JSON.parse(response) as InitializeResult;
    },

    async initialized(): Promise<void> {
      await lspRequest('initialized', {});
    },

    async didOpen(uri: string, text: string, version: number): Promise<void> {
      await lspRequest('textDocument/didOpen', {
        textDocument: { uri, languageId: 'reify', version, text },
      });
    },

    async didClose(uri: string): Promise<void> {
      await lspRequest('textDocument/didClose', {
        textDocument: { uri },
      });
    },

    async didChange(uri: string, text: string, version: number): Promise<void> {
      await lspRequest('textDocument/didChange', {
        textDocument: { uri, version },
        contentChanges: [{ text }],
      });
    },

    async completion(
      uri: string,
      line: number,
      character: number,
    ): Promise<CompletionItem[]> {
      const response = await lspRequest('textDocument/completion', {
        textDocument: { uri },
        position: { line, character },
      });
      const parsed = JSON.parse(response);
      if (Array.isArray(parsed)) {
        return parsed as CompletionItem[];
      }
      // CompletionList format
      if (parsed && Array.isArray(parsed.items)) {
        return parsed.items as CompletionItem[];
      }
      return [];
    },

    async hover(
      uri: string,
      line: number,
      character: number,
    ): Promise<HoverResult | null> {
      const response = await lspRequest('textDocument/hover', {
        textDocument: { uri },
        position: { line, character },
      });
      const parsed = JSON.parse(response);
      if (!parsed || parsed === 'null') return null;
      return parsed as HoverResult;
    },

    async gotoDefinition(
      uri: string,
      line: number,
      character: number,
    ): Promise<Location | null> {
      const response = await lspRequest('textDocument/definition', {
        textDocument: { uri },
        position: { line, character },
      });
      const parsed = JSON.parse(response);
      if (!parsed || parsed === 'null') return null;
      return parsed as Location;
    },

    async documentSymbol(uri: string): Promise<DocumentSymbol[]> {
      // CONTRACT: Reify's LSP server always returns DocumentSymbolResponse::Nested
      // (a DocumentSymbol[] with range / selectionRange / children fields).
      // The alternative SymbolInformation[] shape (which carries `location`
      // instead of range/selectionRange) is NOT supported here — if the server
      // ever changes this, the downstream flattenSymbols / symbolToLocation helpers
      // in commandPaletteFilter.ts must also be updated.
      const response = await lspRequest('textDocument/documentSymbol', {
        textDocument: { uri },
      });
      const parsed = JSON.parse(response);
      if (!Array.isArray(parsed)) return [];
      return parsed as DocumentSymbol[];
    },

    async documentHighlight(
      uri: string,
      line: number,
      character: number,
    ): Promise<DocumentHighlight[]> {
      // Mirrors documentSymbol: a null payload (no resolvable symbol under the
      // cursor) or any non-array shape yields [] so the caller clears highlights.
      const response = await lspRequest('textDocument/documentHighlight', {
        textDocument: { uri },
        position: { line, character },
      });
      const parsed = JSON.parse(response);
      if (!Array.isArray(parsed)) return [];
      return parsed as DocumentHighlight[];
    },

    async prepareRename(
      uri: string,
      line: number,
      character: number,
    ): Promise<PrepareRenameResult | null> {
      // prepareRename is the Invariant-4 refusal gate: the server returns null
      // for any non-renameable position, which the editor must treat as "refuse".
      const response = await lspRequest('textDocument/prepareRename', {
        textDocument: { uri },
        position: { line, character },
      });
      const parsed = JSON.parse(response);
      // A null payload (the Invariant-4 refusal) parses to JS null → !parsed.
      if (!parsed) return null;
      return parsed as PrepareRenameResult;
    },

    async rename(
      uri: string,
      line: number,
      character: number,
      newName: string,
    ): Promise<WorkspaceEdit | null> {
      const response = await lspRequest('textDocument/rename', {
        textDocument: { uri },
        position: { line, character },
        newName,
      });
      const parsed = JSON.parse(response);
      // A null payload (non-renameable / invalid name) parses to JS null → !parsed.
      if (!parsed) return null;
      return parsed as WorkspaceEdit;
    },

    async references(
      uri: string,
      line: number,
      character: number,
      includeDeclaration: boolean,
    ): Promise<Location[]> {
      // The server returns a JSON array of LSP Location[] (declaration ∪ uses),
      // or `null` when the cursor is not on a local value-member binding / the
      // URI is unknown (Ok(None)); treat any non-array as the empty set.
      const response = await lspRequest('textDocument/references', {
        textDocument: { uri },
        position: { line, character },
        context: { includeDeclaration },
      });
      const parsed = JSON.parse(response);
      if (!Array.isArray(parsed)) return [];
      return parsed as Location[];
    },
  };
}
