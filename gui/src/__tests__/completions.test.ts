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
        doc: { lineAt: (pos: number) => ({ number: 1, from: 0, to: 10 }) },
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
});
