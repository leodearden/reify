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
}

// ── Private helpers ────────────────────────────────────────────────────

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
      const response = await lspRequest('textDocument/documentSymbol', {
        textDocument: { uri },
      });
      const parsed = JSON.parse(response);
      if (!Array.isArray(parsed)) return [];
      return parsed as DocumentSymbol[];
    },
  };
}
