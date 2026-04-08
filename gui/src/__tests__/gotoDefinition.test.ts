import { describe, it, expect, vi, beforeEach } from 'vitest';
import { flushMacrotasks, withSuppressedRejectionsAndWarnSpy } from './test-utils';

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

/**
 * Factory for a minimal mock EditorView used by gotoDefinition tests.
 * Overrides are merged per-leaf: only the fields you pass are replaced;
 * sibling defaults are preserved.
 */
function makeMockView(overrides?: {
  posAtCoords?: () => number;
  state?: {
    doc?: {
      lines?: number;
      lineAt?: () => { number: number; from: number; to: number };
      line?: (n: number) => { from: number; to?: number };
    };
  };
  dispatch?: ReturnType<typeof vi.fn>;
  dom?: { isConnected?: boolean };
}) {
  return {
    posAtCoords: overrides?.posAtCoords ?? (() => 5),
    state: {
      doc: {
        lines: overrides?.state?.doc?.lines ?? 100,
        lineAt: overrides?.state?.doc?.lineAt ?? (() => ({ number: 1, from: 0, to: 10 })),
        line: overrides?.state?.doc?.line ?? (() => ({ from: 0, to: 10 })),
      },
    },
    dispatch: overrides?.dispatch ?? vi.fn(),
    dom: {
      isConnected: overrides?.dom?.isConnected ?? true,
    },
  };
}

function makeMouseEvent(overrides?: Partial<MouseEvent>): MouseEvent {
  return {
    ctrlKey: true,
    metaKey: false,
    clientX: 100,
    clientY: 50,
    ...overrides,
  } as MouseEvent;
}

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

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView();

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

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView();

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

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }) } },
    });

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

  it('bare-path currentUri with file:// location.uri for same file dispatches cursor movement', async () => {
    // currentUri is a bare path; LSP returns a file:// URI for the same physical file.
    // isSameFile() should normalize both to the same bare path and recognize them as equal,
    // so the handler dispatches cursor movement instead of calling onNavigate.
    const currentUri = '/project/src/foo.ri';
    const sameFileLocation = {
      uri: 'file:///project/src/foo.ri',
      range: { start: { line: 7, character: 4 }, end: { line: 7, character: 11 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(sameFileLocation));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20 }) } },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // onNavigate should NOT be called — it is the same file after URI normalization
    expect(onNavigate).not.toHaveBeenCalled();
    // view.dispatch SHOULD be called for same-file cursor movement
    expect(mockView.dispatch).toHaveBeenCalledWith({
      selection: { anchor: expect.any(Number) },
      scrollIntoView: true,
    });
  });

  it('partial suffix overlap (/a/foo.ri vs /b/a/foo.ri) triggers cross-file navigation', async () => {
    // currentUri is '/a/foo.ri'; LSP returns 'file:///b/a/foo.ri'.
    // The bare path '/a/foo.ri' is a suffix of '/b/a/foo.ri', but they are different files.
    // isSameFile() must not be fooled by the suffix match — it does exact comparison after
    // normalization, so the handler should call onNavigate instead of dispatching.
    const currentUri = '/a/foo.ri';
    const crossFileLocation = {
      uri: 'file:///b/a/foo.ri',
      range: { start: { line: 3, character: 0 }, end: { line: 3, character: 6 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(crossFileLocation));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20 }) } },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // onNavigate SHOULD be called — different files despite the suffix overlap
    expect(onNavigate).toHaveBeenCalledWith('file:///b/a/foo.ri', 3, 0);
    // view.dispatch should NOT be called (different file)
    expect(mockView.dispatch).not.toHaveBeenCalled();
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

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }) } },
      dom: { isConnected: false },
    });

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

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }) } },
      dom: { isConnected: false },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // Neither onNavigate nor dispatch should be called — editor was destroyed
    expect(onNavigate).not.toHaveBeenCalled();
    expect(mockView.dispatch).not.toHaveBeenCalled();
  });
});

describe('line-bounds guard', () => {
  it('does not dispatch when LSP line exceeds document line count (stale response)', async () => {
    const currentUri = 'file:///current.ri';
    // LSP reports line 10 (0-based), so line+1=11, but doc only has 5 lines
    const staleLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 10, character: 0 }, end: { line: 10, character: 5 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(staleLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    const mockView = makeMockView({
      state: {
        doc: {
          lines: 5,
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // Stale LSP line (11 > 5) should be rejected before reaching doc.line()
    expect(mockView.dispatch).not.toHaveBeenCalled();
  });

  it('does not dispatch when LSP line is negative (malformed response)', async () => {
    const currentUri = 'file:///current.ri';
    // LSP reports line -1 (malformed), doc has 100 lines
    // Without a negative check: -1 + 1 = 0, which is never > 100, so the guard passes
    // doc.line(0) would then throw a RangeError (CodeMirror is 1-indexed)
    const malformedLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: -1, character: 0 }, end: { line: -1, character: 5 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(malformedLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    // doc.line() throws to prove the guard fires BEFORE it is reached.
    // If the guard fires, doc.line() is never called and no console.warn is emitted.
    // If the guard is absent, doc.line() throws, .catch() logs a warning — a false pass.
    const mockView = makeMockView({
      state: { doc: { line: () => { throw new RangeError('line out of range'); } } },
    });

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      mousedownHandler(mockEvent, mockView);
      await flushMacrotasks();

      // Negative line should be rejected by the guard; dispatch must never be called
      expect(mockView.dispatch).not.toHaveBeenCalled();
      // The guard fires before doc.line(), so no warning should be logged
      expect(warnSpy).not.toHaveBeenCalled();
    } finally {
      warnSpy.mockRestore();
    }
  });

  it('dispatches when LSP line is exactly at document boundary (start.line=4, doc.lines=5)', async () => {
    const currentUri = 'file:///current.ri';
    // LSP reports line 4 (0-based), so line+1=5 which equals doc.lines=5; guard 5>5 is false.
    // doc.line(5) is called and returns { from: 80 }, so anchor = 80 + character(0) = 80.
    const boundaryLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 4, character: 0 }, end: { line: 4, character: 3 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(boundaryLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    // line n (1-based) → from = (n-1)*20, to = (n-1)*20+15
    const mockView = makeMockView({
      state: {
        doc: {
          lines: 5,
          line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }),
        },
      },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // Exact boundary (5 > 5 is false) — dispatch should be called with the exact anchor.
    // Asserting anchor: 80 (not expect.any(Number)) so an off-by-one in the line indexing
    // (e.g. doc.line(n) instead of doc.line(n-1)) would fail this test.
    expect(mockView.dispatch).toHaveBeenCalledWith({
      selection: { anchor: 80 },
      scrollIntoView: true,
    });
  });
});

describe('character-bounds guard', () => {
  it('does not dispatch when character offset exceeds line length', async () => {
    const currentUri = 'file:///current.ri';
    // Line has from=100, to=110 → length=10. Character offset 15 exceeds that.
    const location = {
      uri: 'file:///current.ri',
      range: { start: { line: 5, character: 15 }, end: { line: 5, character: 15 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(location));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    // line 6 (1-based): from=100, to=110 → length 10; character 15 > 10
    const mockView = makeMockView({
      state: { doc: { line: () => ({ from: 100, to: 110 }) } },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // Character offset exceeds line length; neither dispatch nor onNavigate should be called
    expect(mockView.dispatch).not.toHaveBeenCalled();
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('dispatches when character offset equals line length (end-of-line boundary)', async () => {
    const currentUri = 'file:///current.ri';
    // Line has from=100, to=110 → length=10. Character offset 10 equals the length.
    // This is a valid end-of-line cursor position and must be accepted.
    const location = {
      uri: 'file:///current.ri',
      range: { start: { line: 5, character: 10 }, end: { line: 5, character: 10 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(location));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    // line 6 (1-based): from=100, to=110 → length 10; character 10 == length (valid)
    const mockView = makeMockView({
      state: { doc: { line: () => ({ from: 100, to: 110 }) } },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // character == line length is valid (end-of-line); dispatch must be called
    // targetPos = from + character = 100 + 10 = 110
    expect(mockView.dispatch).toHaveBeenCalledWith({
      selection: { anchor: 110 },
      scrollIntoView: true,
    });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('dispatches when character offset is 0 on an empty line', async () => {
    const currentUri = 'file:///current.ri';
    // Empty line: from=50, to=50 → length=0. Character offset 0 is the only valid position.
    const location = {
      uri: 'file:///current.ri',
      range: { start: { line: 2, character: 0 }, end: { line: 2, character: 0 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(location));

    const onNavigate = vi.fn();
    const ext = reifyGotoDefinition(currentUri, onNavigate) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    // line 3 (1-based): from=50, to=50 → empty line; character 0 == length 0 (valid)
    const mockView = makeMockView({
      state: { doc: { line: () => ({ from: 50, to: 50 }) } },
    });

    mousedownHandler(mockEvent, mockView);
    await flushMacrotasks();

    // character 0 on an empty line is valid; dispatch must be called with anchor 50
    expect(mockView.dispatch).toHaveBeenCalledWith({
      selection: { anchor: 50 },
      scrollIntoView: true,
    });
    expect(onNavigate).not.toHaveBeenCalled();
  });
});

describe('.catch() error handler', () => {
  it('logs a warning when doc.line() throws RangeError (no unhandled rejection)', async () => {
    const currentUri = 'file:///current.ri';
    // Same-file response so the code reaches doc.line()
    const sameFileLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 5, character: 0 }, end: { line: 5, character: 5 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(sameFileLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    const rangeError = new RangeError('line out of range');
    // Simulate doc.line() throwing a RangeError
    const mockView = makeMockView({
      state: { doc: { line: () => { throw rangeError; } } },
    });

    await withSuppressedRejectionsAndWarnSpy(async (warnSpy) => {
      mousedownHandler(mockEvent, mockView);
      await flushMacrotasks();
      // .catch() should log a warning with the expected prefix and the error
      expect(warnSpy).toHaveBeenCalledWith('gotoDefinition: failed to apply result', rangeError);
      // dispatch should not have been called (line() threw before it could be called)
      expect(mockView.dispatch).not.toHaveBeenCalled();
    });
  });

  it('logs a warning when view.dispatch() throws (catch covers full .then() body)', async () => {
    const currentUri = 'file:///current.ri';
    // Same-file response with a valid line so the code reaches view.dispatch()
    const sameFileLocation = {
      uri: 'file:///current.ri',
      range: { start: { line: 2, character: 0 }, end: { line: 2, character: 5 } },
    };
    mockInvoke.mockResolvedValue(JSON.stringify(sameFileLocation));

    const ext = reifyGotoDefinition(currentUri) as any;
    const mousedownHandler = ext.handlers.mousedown;

    const mockEvent = makeMouseEvent();

    const dispatchError = new Error('dispatch blew up');
    // dispatch itself throws — this error should be caught by .catch()
    const mockView = makeMockView({
      state: { doc: { line: (n: number) => ({ from: (n - 1) * 20, to: (n - 1) * 20 + 15 }) } },
      dispatch: vi.fn().mockImplementation(() => { throw dispatchError; }),
    });

    await withSuppressedRejectionsAndWarnSpy(async (warnSpy) => {
      mousedownHandler(mockEvent, mockView);
      await flushMacrotasks();
      // .catch() should log a warning — proving it covers dispatch(), not just doc.line()
      expect(warnSpy).toHaveBeenCalledWith('gotoDefinition: failed to apply result', dispatchError);
      // dispatch WAS called (it just threw)
      expect(mockView.dispatch).toHaveBeenCalled();
    });
  });
});
