import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { reifyCompletionSource } from '../editor/completions';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('reifyCompletionSource', () => {
  it('returns a CompletionSource function', () => {
    const source = reifyCompletionSource('file:///test.ri');
    expect(typeof source).toBe('function');
  });

  it('calls lsp_request with textDocument/completion and returns CompletionResult', async () => {
    const mockItems = [
      { label: 'width', kind: 6, detail: 'Scalar = 80mm' },
      { label: 'height', kind: 6, detail: 'Scalar = 100mm' },
    ];
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    const source = reifyCompletionSource('file:///test.ri');

    // Create a minimal CompletionContext mock
    const context = {
      state: {
        doc: {
          lineAt: (pos: number) => ({ number: 1, from: 0, to: 10 }),
          sliceString: (from: number, to: number) => 'hello world'.slice(from, to),
        },
        selection: { main: { head: 5 } },
      },
      pos: 5,
      explicit: true,
    } as any;

    const result = await source(context);

    expect(mockInvoke).toHaveBeenCalledWith('lsp_request', {
      method: 'textDocument/completion',
      params: expect.any(String),
    });

    expect(result).not.toBeNull();
    expect(result!.options).toHaveLength(2);
    expect(result!.options[0].label).toBe('width');
  });

  it('returns null when no completions are available', async () => {
    mockInvoke.mockResolvedValue(JSON.stringify([]));

    const source = reifyCompletionSource('file:///test.ri');

    const context = {
      state: {
        doc: { lineAt: (pos: number) => ({ number: 1, from: 0, to: 10 }) },
        selection: { main: { head: 5 } },
      },
      pos: 5,
      explicit: true,
    } as any;

    const result = await source(context);

    // Empty completions should return null
    expect(result).toBeNull();
  });

  it('accepts a () => string getter and uses current URI for each request', async () => {
    let currentUri = 'file:///first.ri';
    const mockItems = [{ label: 'x', kind: 6 }];
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    const source = reifyCompletionSource(() => currentUri);

    const context = {
      state: {
        doc: { lineAt: () => ({ number: 1, from: 0, to: 10 }) },
        selection: { main: { head: 5 } },
      },
      pos: 5,
      explicit: true,
    } as any;

    // First request uses first URI
    await source(context);
    let params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///first.ri');

    // Switch URI
    currentUri = 'file:///second.ri';
    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    // Second request uses updated URI
    await source(context);
    params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///second.ri');
  });

  it('completion result.from scans backward to word start (cursor at end of word)', async () => {
    const mockItems = [{ label: 'width', kind: 6 }];
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    const source = reifyCompletionSource('file:///test.ri');

    // Simulating: doc text is 'wid', cursor at pos=3 (end of 'wid')
    // line.from=0, line.to=3
    const context = {
      state: {
        doc: {
          lineAt: () => ({ number: 1, from: 0, to: 3 }),
          sliceString: (from: number, to: number) => 'wid'.slice(from, to),
        },
        selection: { main: { head: 3 } },
      },
      pos: 3,
      explicit: true,
    } as any;

    const result = await source(context);
    expect(result).not.toBeNull();
    // from should be 0 (start of 'wid'), NOT 3 (cursor position)
    expect(result!.from).toBe(0);
  });

  it('completion result.from scans backward past non-identifier chars', async () => {
    const mockItems = [{ label: 'width', kind: 6 }];
    mockInvoke.mockResolvedValue(JSON.stringify(mockItems));

    const source = reifyCompletionSource('file:///test.ri');

    // Simulating: doc text is 'x = wid', cursor at pos=7 (end of 'wid')
    // line.from=0, line.to=7
    const context = {
      state: {
        doc: {
          lineAt: () => ({ number: 1, from: 0, to: 7 }),
          sliceString: (from: number, to: number) => 'x = wid'.slice(from, to),
        },
        selection: { main: { head: 7 } },
      },
      pos: 7,
      explicit: true,
    } as any;

    const result = await source(context);
    expect(result).not.toBeNull();
    // from should be 4 (start of 'wid'), NOT 7 (cursor position)
    expect(result!.from).toBe(4);
  });
});
