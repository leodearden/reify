import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

// Mock @codemirror/view to avoid DOM dependencies
vi.mock('@codemirror/view', () => ({
  hoverTooltip: (handler: Function) => ({ handler }),
  EditorView: {},
}));

import { invoke } from '@tauri-apps/api/core';
import { reifyHoverTooltip } from '../editor/hover';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('reifyHoverTooltip', () => {
  it('returns a hoverTooltip extension', () => {
    const ext = reifyHoverTooltip('file:///test.ri');
    expect(ext).toBeDefined();
  });

  it('accepts a () => string getter and uses current URI for each request', async () => {
    let currentUri = 'file:///first.ri';
    const mockHover = {
      contents: { kind: 'plaintext', value: 'width: Scalar' },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockHover));

    const ext = reifyHoverTooltip(() => currentUri) as any;
    const handler = ext.handler;

    const mockView = {
      state: {
        doc: { lineAt: () => ({ number: 1, from: 0, to: 10 }) },
      },
    };

    // First call uses first URI
    await handler(mockView, 5, 1);
    let params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///first.ri');

    // Switch URI
    currentUri = 'file:///second.ri';
    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue(JSON.stringify(mockHover));

    // Second call uses updated URI
    await handler(mockView, 5, 1);
    params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///second.ri');
  });

  it('tooltip create() returns DOM with cm-lsp-hover class and no inline styles', async () => {
    const mockHover = {
      contents: { kind: 'plaintext', value: 'width: Scalar = 80mm' },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockHover));

    const ext = reifyHoverTooltip('file:///test.ri') as any;
    const handler = ext.handler;

    const mockView = {
      state: {
        doc: { lineAt: () => ({ number: 1, from: 0, to: 10 }) },
      },
    };

    const tooltip = await handler(mockView, 5, 1);
    expect(tooltip).not.toBeNull();

    // Invoke the create() factory to get DOM
    const { dom } = tooltip.create();

    // Verify className
    expect(dom.className).toBe('cm-lsp-hover');
    // Verify textContent
    expect(dom.textContent).toBe('width: Scalar = 80mm');

    // Verify no inline styles (CSS handles styling now)
    expect(dom.style.padding).toBe('');
    expect(dom.style.maxWidth).toBe('');
    expect(dom.style.whiteSpace).toBe('');
  });
});
