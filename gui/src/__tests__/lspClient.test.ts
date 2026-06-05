import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { createLspClient } from '../editor/lspClient';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('createLspClient', () => {
  it('creates a client object with LSP methods', () => {
    const client = createLspClient();
    expect(client).toBeDefined();
    expect(typeof client.initialize).toBe('function');
    expect(typeof client.didOpen).toBe('function');
    expect(typeof client.didChange).toBe('function');
    expect(typeof client.completion).toBe('function');
    expect(typeof client.hover).toBe('function');
    expect(typeof client.gotoDefinition).toBe('function');
  });

  it('initialize sends lsp_request with initialize method', async () => {
    mockInvoke.mockResolvedValue(
      JSON.stringify({
        capabilities: {
          completionProvider: {},
          hoverProvider: true,
          definitionProvider: true,
        },
      }),
    );

    const client = createLspClient();
    const result = await client.initialize();

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'initialize',
      params: expect.any(String),
    });
    expect(result.capabilities).toBeDefined();
    expect(result.capabilities.completionProvider).toBeDefined();
  });

  it('didOpen sends textDocument/didOpen notification', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    await client.didOpen('file:///test.ri', 'structure Foo {}', 1);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/didOpen',
      params: expect.stringContaining('"uri":"file:///test.ri"'),
    });
  });

  it('didChange sends textDocument/didChange notification', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    await client.didChange('file:///test.ri', 'structure Bar {}', 2);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/didChange',
      params: expect.stringContaining('"uri":"file:///test.ri"'),
    });
  });

  it('completion sends textDocument/completion and returns items', async () => {
    const mockItems = [
      { label: 'width', kind: 6 },
      { label: 'height', kind: 6 },
    ];
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    const client = createLspClient();
    const result = await client.completion('file:///test.ri', 1, 0);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/completion',
      params: expect.any(String),
    });
    expect(result).toHaveLength(2);
    expect(result[0].label).toBe('width');
  });

  it('hover sends textDocument/hover and returns hover info', async () => {
    const mockHover = {
      contents: { kind: 'markdown', value: '**width**: Scalar = 80mm' },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockHover));

    const client = createLspClient();
    const result = await client.hover('file:///test.ri', 1, 10);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/hover',
      params: expect.any(String),
    });
    expect(result).not.toBeNull();
    expect(result!.contents).toBeDefined();
  });

  it('gotoDefinition sends textDocument/definition and returns location', async () => {
    const mockLocation = {
      uri: 'file:///test.ri',
      range: { start: { line: 2, character: 4 }, end: { line: 2, character: 9 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockLocation));

    const client = createLspClient();
    const result = await client.gotoDefinition('file:///test.ri', 9, 15);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/definition',
      params: expect.any(String),
    });
    expect(result).not.toBeNull();
    expect(result!.uri).toBe('file:///test.ri');
  });

  it('initialized sends initialized notification via lsp_request', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    await client.initialized();

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'initialized',
      params: expect.any(String),
    });

    // Verify params is empty object
    const callArgs = mockInvoke.mock.calls[0];
    const params = JSON.parse((callArgs[1] as { params: string }).params);
    expect(params).toEqual({});
  });

  it('didClose sends textDocument/didClose notification with correct URI', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    await client.didClose('file:///project/main.ri');

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/didClose',
      params: expect.stringContaining('"uri":"file:///project/main.ri"'),
    });
  });

  it('createLspClient() exposes a documentSymbol function', () => {
    const client = createLspClient();
    expect(typeof client.documentSymbol).toBe('function');
  });

  it('documentSymbol sends textDocument/documentSymbol with textDocument wrapper', async () => {
    mockInvoke.mockResolvedValue('[]');

    const client = createLspClient();
    await client.documentSymbol('file:///test.ri');

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/documentSymbol',
      params: expect.any(String),
    });
    const callArgs = mockInvoke.mock.calls[0];
    const params = JSON.parse((callArgs[1] as { params: string }).params);
    expect(params).toHaveProperty('textDocument');
    expect(params.textDocument).toHaveProperty('uri', 'file:///test.ri');
  });

  it('documentSymbol returns an array of DocumentSymbol objects from a nested response', async () => {
    const mockSymbols = [
      {
        name: 'Bracket',
        kind: 5,
        range: { start: { line: 0, character: 0 }, end: { line: 10, character: 1 } },
        selectionRange: { start: { line: 0, character: 10 }, end: { line: 0, character: 17 } },
        children: [
          {
            name: 'width',
            kind: 13,
            range: { start: { line: 1, character: 2 }, end: { line: 1, character: 14 } },
            selectionRange: { start: { line: 1, character: 2 }, end: { line: 1, character: 7 } },
          },
        ],
      },
    ];
    mockInvoke.mockResolvedValue(JSON.stringify(mockSymbols));

    const client = createLspClient();
    const result = await client.documentSymbol('file:///test.ri');

    expect(result).toHaveLength(1);
    expect(result[0].name).toBe('Bracket');
    expect(result[0].children).toHaveLength(1);
    expect(result[0].children![0].name).toBe('width');
    expect(result[0].selectionRange).toBeDefined();
  });

  it('documentSymbol returns [] when the response is null (unknown URI -> Ok(None))', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    const result = await client.documentSymbol('file:///unknown.ri');

    expect(result).toEqual([]);
  });

  it('documentSymbol returns [] when the response is not an array', async () => {
    // DocumentSymbolResponse::Flat would be an object; treat non-array as empty
    mockInvoke.mockResolvedValue(JSON.stringify({ items: [] }));

    const client = createLspClient();
    const result = await client.documentSymbol('file:///test.ri');

    expect(result).toEqual([]);
  });

  // --- task 4203 γ: prepareRename / rename ---

  it('createLspClient() exposes prepareRename and rename functions', () => {
    const client = createLspClient();
    expect(typeof client.prepareRename).toBe('function');
    expect(typeof client.rename).toBe('function');
  });

  it('prepareRename sends textDocument/prepareRename with the position and returns the target', async () => {
    const mockTarget = {
      range: { start: { line: 7, character: 17 }, end: { line: 7, character: 22 } },
      placeholder: 'width',
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockTarget));

    const client = createLspClient();
    const result = await client.prepareRename('file:///test.ri', 7, 17);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/prepareRename',
      params: expect.any(String),
    });
    const callArgs = mockInvoke.mock.calls[0];
    const params = JSON.parse((callArgs[1] as { params: string }).params);
    expect(params).toEqual({
      textDocument: { uri: 'file:///test.ri' },
      position: { line: 7, character: 17 },
    });
    expect(result).not.toBeNull();
    expect(result!.placeholder).toBe('width');
    expect(result!.range.start).toEqual({ line: 7, character: 17 });
  });

  it('prepareRename returns null when the response is null (Invariant-4 refusal)', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    const result = await client.prepareRename('file:///test.ri', 0, 0);

    expect(result).toBeNull();
  });

  it('rename sends textDocument/rename with newName and returns the WorkspaceEdit', async () => {
    const mockEdit = {
      changes: {
        'file:///test.ri': [
          {
            range: { start: { line: 1, character: 10 }, end: { line: 1, character: 15 } },
            newText: 'girth',
          },
          {
            range: { start: { line: 7, character: 17 }, end: { line: 7, character: 22 } },
            newText: 'girth',
          },
        ],
      },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockEdit));

    const client = createLspClient();
    const result = await client.rename('file:///test.ri', 7, 17, 'girth');

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/rename',
      params: expect.any(String),
    });
    const callArgs = mockInvoke.mock.calls[0];
    const params = JSON.parse((callArgs[1] as { params: string }).params);
    expect(params).toEqual({
      textDocument: { uri: 'file:///test.ri' },
      position: { line: 7, character: 17 },
      newName: 'girth',
    });
    expect(result).not.toBeNull();
    expect(result!.changes!['file:///test.ri']).toHaveLength(2);
  });

  it('rename returns null when the response is null', async () => {
    mockInvoke.mockResolvedValue('null');

    const client = createLspClient();
    const result = await client.rename('file:///test.ri', 0, 0, 'girth');

    expect(result).toBeNull();
  });
});
