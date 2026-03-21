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
});
