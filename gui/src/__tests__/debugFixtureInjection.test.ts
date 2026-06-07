/**
 * Unit tests for the F1 debug bridge handlers:
 *   inject_diagnostics, reset_app_state, element_screenshot
 *
 * All three are frontend-mediated (reached via the default dispatch arm →
 * query_frontend → bridge.ts buildHandlers).  Tests mock @tauri-apps/api/*
 * and html-to-image; no real three.js or DOM rendering is required.
 *
 * Pattern mirrors debugCanvasInteraction.test.ts:
 *   vi.mock mocks → makeStores() stub → initDebugBridge → captured listen
 *   handler → dispatchAndGetResult.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ─── Mocks ────────────────────────────────────────────────────────────────────

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
  LogicalSize: class LogicalSize {
    constructor(w: number, h: number) {
      (this as any).width = w;
      (this as any).height = h;
    }
  },
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,FULLWINDOW'),
}));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { toPng } from 'html-to-image';
import { initDebugBridge } from '../debug/bridge';
import { makeViewStateStoreMock } from './debugBridgeTestHelpers';
import type { DebugStores } from '../debug/types';

type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

// ─── makeStores ───────────────────────────────────────────────────────────────

function makeStores(): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
        compileDiagnostics: [],
        tessellationDiagnostics: [],
      },
      initFromState: vi.fn(),
      setCompileDiagnostics: vi.fn(),
      setTessellationDiagnostics: vi.fn(),
    },
    editor: {
      state: {
        openFiles: [],
        activeFile: null,
        dirtyFiles: [],
        externallyChanged: [],
        cursorPosition: null,
      },
      openFile: vi.fn(),
      closeFile: vi.fn(),
    },
    selection: {
      state: {
        selectedEntity: null,
        selectedEntities: [],
        anchorEntity: null,
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
      clearSelection: vi.fn(),
      toggleSelect: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
    viewState: makeViewStateStoreMock(),
    layout: {
      state: {
        editorWidth: 300,
        sideWidth: 300,
        designTreeHeight: 160,
        propertyHeight: 200,
        constraintHeight: 140,
      },
      setEditorWidth: vi.fn(),
      setSideWidth: vi.fn(),
      setDesignTreeHeight: vi.fn(),
      setPropertyHeight: vi.fn(),
      setConstraintHeight: vi.fn(),
    },
  };
}

// ─── dispatchAndGetResult ─────────────────────────────────────────────────────

async function dispatchAndGetResult(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown> = {},
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  if (!responseCall) return undefined;
  const payload = responseCall[1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

// ─── inject_diagnostics ───────────────────────────────────────────────────────

describe('inject_diagnostics: bridge handler', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let stores: DebugStores;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;

    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('compile source: routes to setCompileDiagnostics with normalized entry, returns {ok,count,source}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'inject_diagnostics', {
      diagnostics: [{ severity: 'Error', message: 'boom' }],
      source: 'compile',
    }) as any;

    // Return shape
    expect(result.ok).toBe(true);
    expect(result.count).toBe(1);
    expect(result.source).toBe('compile');

    // Routes to the compile setter
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).not.toHaveBeenCalled();

    // Normalized entry: preserves caller fields, defaults omitted positional fields
    const [normalized] = vi.mocked(stores.engine.setCompileDiagnostics).mock.calls[0][0];
    expect(normalized.severity).toBe('Error');
    expect(normalized.message).toBe('boom');
    expect(typeof normalized.file_path).toBe('string');
    expect(normalized.file_path.length).toBeGreaterThan(0);
    expect(typeof normalized.line).toBe('number');
    expect(typeof normalized.column).toBe('number');
    expect(typeof normalized.end_line).toBe('number');
    expect(typeof normalized.end_column).toBe('number');
    // code is either a string or null (not undefined)
    expect(normalized.code === null || typeof normalized.code === 'string').toBe(true);
  });

  it('tessellation source: routes to setTessellationDiagnostics, NOT setCompileDiagnostics', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 2, 'inject_diagnostics', {
      diagnostics: [{ severity: 'Warning', message: 'tess warning' }],
      source: 'tessellation',
    }) as any;

    expect(result.ok).toBe(true);
    expect(result.source).toBe('tessellation');
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).not.toHaveBeenCalled();
  });

  it('default source (omitted): routes to setCompileDiagnostics', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 3, 'inject_diagnostics', {
      diagnostics: [{ severity: 'Info', message: 'hello' }],
    }) as any;

    expect(result.ok).toBe(true);
    expect(result.source).toBe('compile');
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).not.toHaveBeenCalled();
  });

  it('missing diagnostics: returns {error}, neither setter called', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 4, 'inject_diagnostics', {}) as any;

    expect(typeof result.error).toBe('string');
    expect(result.error.length).toBeGreaterThan(0);
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).not.toHaveBeenCalled();
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).not.toHaveBeenCalled();
  });

  it('non-array diagnostics: returns {error}, neither setter called', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 5, 'inject_diagnostics', {
      diagnostics: 'not-an-array',
    }) as any;

    expect(typeof result.error).toBe('string');
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).not.toHaveBeenCalled();
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).not.toHaveBeenCalled();
  });
});

// ─── reset_app_state ──────────────────────────────────────────────────────────

describe('reset_app_state: bridge handler', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let stores: DebugStores;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;

    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    // Stub with two open files and a selected entity
    stores = makeStores();
    (stores.editor.state as any).openFiles = [
      { path: 'a.ri', content: '' },
      { path: 'b.ri', content: '' },
    ];
    (stores.selection.state as any).selectedEntity = 'some/entity';

    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('calls closeFile for each open file path', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'reset_app_state') as any;

    expect(result.ok).toBe(true);
    expect(vi.mocked(stores.editor.closeFile)).toHaveBeenCalledTimes(2);
    expect(vi.mocked(stores.editor.closeFile)).toHaveBeenCalledWith('a.ri');
    expect(vi.mocked(stores.editor.closeFile)).toHaveBeenCalledWith('b.ri');
  });

  it('calls clearSelection once', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 2, 'reset_app_state') as any;

    expect(result.ok).toBe(true);
    expect(vi.mocked(stores.selection.clearSelection)).toHaveBeenCalledTimes(1);
  });

  it('calls viewState.resetToDefaultView once', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 3, 'reset_app_state') as any;

    expect(result.ok).toBe(true);
    expect(vi.mocked(stores.viewState.resetToDefaultView)).toHaveBeenCalledTimes(1);
  });

  it('clears diagnostics via both engine setters with empty arrays', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 4, 'reset_app_state') as any;

    expect(result.ok).toBe(true);
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.engine.setCompileDiagnostics)).toHaveBeenCalledWith([]);
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.engine.setTessellationDiagnostics)).toHaveBeenCalledWith([]);
  });

  it('resets each layout dimension to DEFAULT_* value', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 5, 'reset_app_state') as any;

    expect(result.ok).toBe(true);
    // DEFAULT_EDITOR_WIDTH=300, DEFAULT_SIDE_WIDTH=300, DEFAULT_DESIGN_TREE_HEIGHT=160,
    // DEFAULT_PROPERTY_HEIGHT=200, DEFAULT_CONSTRAINT_HEIGHT=140
    expect(vi.mocked(stores.layout.setEditorWidth)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.layout.setEditorWidth)).toHaveBeenCalledWith(300);
    expect(vi.mocked(stores.layout.setSideWidth)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.layout.setSideWidth)).toHaveBeenCalledWith(300);
    expect(vi.mocked(stores.layout.setDesignTreeHeight)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.layout.setDesignTreeHeight)).toHaveBeenCalledWith(160);
    expect(vi.mocked(stores.layout.setPropertyHeight)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.layout.setPropertyHeight)).toHaveBeenCalledWith(200);
    expect(vi.mocked(stores.layout.setConstraintHeight)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(stores.layout.setConstraintHeight)).toHaveBeenCalledWith(140);
  });
});

// ─── element_screenshot ───────────────────────────────────────────────────────

describe('element_screenshot: bridge handler', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let stores: DebugStores;
  let drawImageSpy: ReturnType<typeof vi.fn>;
  let originalCreateElement: typeof document.createElement;
  let originalImage: typeof Image;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;

    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    // Stub the offscreen canvas: capture drawImage calls and return predictable data URL.
    drawImageSpy = vi.fn();
    originalCreateElement = document.createElement.bind(document);
    vi.spyOn(document, 'createElement').mockImplementation((tag: string, ...rest: any[]) => {
      if (tag === 'canvas') {
        const fakeCanvas = {
          width: 0,
          height: 0,
          getContext: () => ({ drawImage: drawImageSpy }),
          toDataURL: () => 'data:image/png;base64,CROP',
        };
        return fakeCanvas as unknown as HTMLCanvasElement;
      }
      return originalCreateElement(tag, ...rest);
    });

    // Stub global Image so that setting .src fires onload synchronously.
    // IMPORTANT: do NOT add `src` as a class field — class fields create own
    // properties on each instance that shadow the prototype getter/setter.
    originalImage = (globalThis as any).Image;
    class FakeImage {
      onload: (() => void) | null = null;
      onerror: ((err?: unknown) => void) | null = null;
    }
    Object.defineProperty(FakeImage.prototype, 'src', {
      configurable: true,
      set(_v: string) {
        // Call onload synchronously so the handler Promise resolves in this tick.
        if ((this as any).onload) (this as any).onload();
      },
      get() { return ''; },
    });
    (globalThis as any).Image = FakeImage;

    // Reset devicePixelRatio to 1 as default; individual tests may override.
    Object.defineProperty(window, 'devicePixelRatio', {
      configurable: true,
      writable: true,
      value: 1,
    });

    stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.restoreAllMocks();
    (globalThis as any).Image = originalImage;
  });

  it('missing testId: returns {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'element_screenshot', {}) as any;

    expect(typeof result.error).toBe('string');
    expect(result.error.length).toBeGreaterThan(0);
  });

  it('element not found: returns {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 2, 'element_screenshot', {
      testId: 'does-not-exist-xyzzy',
    }) as any;

    expect(typeof result.error).toBe('string');
    expect(result.error).toContain('does-not-exist-xyzzy');
  });

  it('zero-area element (width 0): returns {error}', async () => {
    // Insert a zero-width element into the DOM.
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'zero-area-el');
    document.body.appendChild(el);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, width: 0, height: 50,
      top: 0, right: 0, bottom: 50, left: 0,
      toJSON: () => ({}),
    });

    try {
      const result = await dispatchAndGetResult(capturedHandler!, 3, 'element_screenshot', {
        testId: 'zero-area-el',
      }) as any;
      expect(typeof result.error).toBe('string');
    } finally {
      document.body.removeChild(el);
    }
  });

  it('happy path: DPR=2, crops with scaled drawImage, returns {data}', async () => {
    // Set devicePixelRatio to 2
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, writable: true, value: 2 });

    // Insert a visible element into the DOM.
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'crop-target');
    document.body.appendChild(el);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
      x: 10, y: 20, width: 100, height: 50,
      top: 20, right: 110, bottom: 70, left: 10,
      toJSON: () => ({}),
    });

    // Reset toPng mock to return deterministic full-window URL.
    vi.mocked(toPng).mockResolvedValue('data:image/png;base64,FULLWINDOW');

    try {
      const result = await dispatchAndGetResult(capturedHandler!, 4, 'element_screenshot', {
        testId: 'crop-target',
      }) as any;

      // Should return the cropped data URL from fakeCanvas.toDataURL()
      expect(result.data).toBe('data:image/png;base64,CROP');

      // toPng should have been called on document.documentElement
      expect(vi.mocked(toPng)).toHaveBeenCalledWith(document.documentElement, expect.objectContaining({ cacheBust: true }));

      // drawImage should have been called with DPR-scaled source rect
      // sx = x*dpr = 10*2=20, sy = y*dpr = 20*2=40, sw = w*dpr = 100*2=200, sh = h*dpr = 50*2=100
      expect(drawImageSpy).toHaveBeenCalledTimes(1);
      const drawArgs = drawImageSpy.mock.calls[0];
      // drawImage(img, sx, sy, sw, sh, dx, dy, dw, dh)
      expect(drawArgs[1]).toBe(20);   // sx = x*dpr
      expect(drawArgs[2]).toBe(40);   // sy = y*dpr
      expect(drawArgs[3]).toBe(200);  // sw = w*dpr
      expect(drawArgs[4]).toBe(100);  // sh = h*dpr
    } finally {
      document.body.removeChild(el);
    }
  });

  it('oversized output: returns {error, size, limit}', async () => {
    // Make the canvas return a huge data URL
    vi.spyOn(document, 'createElement').mockImplementation((tag: string, ...rest: any[]) => {
      if (tag === 'canvas') {
        const bigData = 'x'.repeat(16 * 1024 * 1024 + 1);
        return {
          width: 0,
          height: 0,
          getContext: () => ({ drawImage: drawImageSpy }),
          toDataURL: () => `data:image/png;base64,${bigData}`,
        } as unknown as HTMLCanvasElement;
      }
      return originalCreateElement(tag, ...rest);
    });

    const el = document.createElement('div');
    el.setAttribute('data-testid', 'oversized-el');
    document.body.appendChild(el);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, width: 100, height: 100,
      top: 0, right: 100, bottom: 100, left: 0,
      toJSON: () => ({}),
    });

    try {
      const result = await dispatchAndGetResult(capturedHandler!, 5, 'element_screenshot', {
        testId: 'oversized-el',
      }) as any;

      expect(typeof result.error).toBe('string');
      expect(typeof result.size).toBe('number');
      expect(typeof result.limit).toBe('number');
      expect(result.size).toBeGreaterThan(result.limit);
    } finally {
      document.body.removeChild(el);
    }
  });
});
