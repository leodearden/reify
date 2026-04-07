import { describe, it, expect, vi, beforeEach } from 'vitest';
import { flushMacrotasks } from './test-utils';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

// Mock @codemirror/view and @codemirror/state to avoid DOM dependencies
vi.mock('@codemirror/view', () => ({
  EditorView: {
    domEventHandlers: (handlers: Record<string, Function>) => ({ handlers }),
  },
}));

vi.mock('@codemirror/state', () => ({
  // Minimal mock
}));

import { invoke } from '@tauri-apps/api/core';
import { reifyGotoDefinition } from '../editor/gotoDefinition';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('reifyGotoDefinition', () => {
  it('returns an extension', () => {
    const ext = reifyGotoDefinition('file:///test.ri');
    expect(ext).toBeDefined();
  });

  it('accepts a () => string getter and uses current URI for each request', async () => {
    let currentUri = 'file:///first.ri';
    const mockLocation = {
      uri: 'file:///first.ri',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(mockLocation));

    const ext = reifyGotoDefinition(() => currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = {
      ctrlKey: true,
      metaKey: false,
      clientX: 100,
      clientY: 50,
    } as MouseEvent;

    const mockView = {
      posAtCoords: () => 5,
      state: {
        doc: {
          lines: 100,
          lineAt: () => ({ number: 1, from: 0, to: 10 }),
          line: () => ({ from: 0 }),
        },
      },
      dispatch: vi.fn(),
      dom: { isConnected: true },
    };

    // First call uses first URI
    mousedownHandler(mockEvent, mockView);
    // Wait for the async requestDefinition to complete
    await flushMacrotasks();
    let params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///first.ri');

    // Switch URI
    currentUri = 'file:///second.ri';
    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue(
      JSON.stringify({
        uri: 'file:///second.ri',
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
      }),
    );

    // Second call uses updated URI
    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();
    params = JSON.parse((mockInvoke.mock.calls[0][1] as { params: string }).params);
    expect(params.textDocument.uri).toBe('file:///second.ri');
  });
});

describe('cross-file goto-definition (onNavigate)', () => {
  it('calls onNavigate callback when definition is in a different file', async () => {
    const currentUri = 'file:///current.ri';
    const crossFileLocation = {
      uri: 'file:///other.ri',
      range: { start: { line: 5, character: 2 }, end: { line: 5, character: 10 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(crossFileLocation));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = {
      ctrlKey: true,
      metaKey: false,
      clientX: 100,
      clientY: 50,
    } as MouseEvent;

    const mockView = {
      posAtCoords: () => 5,
      state: {
        doc: {
          lines: 100,
          lineAt: () => ({ number: 1, from: 0, to: 10 }),
          line: () => ({ from: 0 }),
        },
      },
      dispatch: vi.fn(),
      dom: { isConnected: true },
    };

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // onNavigate should be called with the cross-file URI, line, character
    expect(onNavigate).toHaveBeenCalledWith('file:///other.ri', 5, 2);
    // view.dispatch should NOT be called (different file)
    expect(mockView.dispatch).not.toHaveBeenCalled();
  });

  it('same-file definition dispatches cursor movement without calling onNavigate', async () => {
    const currentUri = 'file:///current.ri';
    const sameFileLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 10, character: 3 }, end: { line: 10, character: 8 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(sameFileLocation));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = {
      ctrlKey: true,
      metaKey: false,
      clientX: 100,
      clientY: 50,
    } as MouseEvent;

    const mockView = {
      posAtCoords: () => 5,
      state: {
        doc: {
          lines: 100,
          lineAt: () => ({ number: 1, from: 0, to: 10 }),
          line: (n: number) => ({ from: (n - 1) * 20 }),
        },
      },
      dispatch: vi.fn(),
      dom: { isConnected: true },
    };

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // onNavigate should NOT be called (same file)
    expect(onNavigate).not.toHaveBeenCalled();
    // view.dispatch should be called for same-file navigation
    expect(mockView.dispatch).toHaveBeenCalledWith({
      selection: { anchor: expect.any(Number) },
      scrollIntoView: true,
    });
  });
});

describe('isConnected guard', () => {
  it('does not dispatch when view.dom.isConnected is false (editor destroyed)', async () => {
    const currentUri = 'file:///current.ri';
    const sameFileLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 5, character: 2 }, end: { line: 5, character: 10 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(sameFileLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = {
      ctrlKey: true,
      metaKey: false,
      clientX: 100,
      clientY: 50,
    } as MouseEvent;

    const mockView = {
      posAtCoords: () => 5,
      state: {
        doc: {
          lines: 100,
          lineAt: () => ({ number: 1, from: 0, to: 10 }),
          line: (n: number) => ({ from: (n - 1) * 20 }),
        },
      },
      dispatch: vi.fn(),
      dom: { isConnected: false },
    };

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // view.dispatch must NOT be called — editor was destroyed before response arrived
    expect(mockView.dispatch).not.toHaveBeenCalled();
  });

  it('does not call onNavigate when view.dom.isConnected is false (cross-file case)', async () => {
    const currentUri = 'file:///current.ri';
    const crossFileLocation = {
      uri: 'file:///other.ri',
      range: { start: { line: 3, character: 1 }, end: { line: 3, character: 7 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(crossFileLocation));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = {
      ctrlKey: true,
      metaKey: false,
      clientX: 100,
      clientY: 50,
    } as MouseEvent;

    const mockView = {
      posAtCoords: () => 5,
      state: {
        doc: {
          lines: 100,
          lineAt: () => ({ number: 1, from: 0, to: 10 }),
          line: (n: number) => ({ from: (n - 1) * 20 }),
        },
      },
      dispatch: vi.fn(),
      dom: { isConnected: false },
    };

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // Neither onNavigate nor dispatch should be called — editor was destroyed
    expect(onNavigate).not.toHaveBeenCalled();
    expect(mockView.dispatch).not.toHaveBeenCalled();
  });
});
