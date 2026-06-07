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
