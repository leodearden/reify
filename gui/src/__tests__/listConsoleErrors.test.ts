/**
 * Unit tests for the list_console_errors debug bridge handler.
 * Mirrors the waitForIdle.test.ts / debugBridge.test.ts harness:
 * - vi.mock @tauri-apps/api/event listen
 * - vi.mock @tauri-apps/api/core invoke
 * - vi.mock three
 * - capture the debug-request handler via the listen mock
 * - dispatchAndGetResult parses the debug_response invoke payload
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('three', () => ({
  Box3: class { expandByObject() {} isEmpty() { return true; } },
  Vector3: class {},
}));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import { installConsoleErrorCapture, clearConsoleErrors } from '../debug/consoleErrors';
import type { DebugStores } from '../debug/types';
import type { ViewStateStore } from '../stores/viewStateStore';

type DebugRequestHandler = (event: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

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
    viewState: { resetToDefaultView: vi.fn() } as unknown as ViewStateStore,
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

/** Dispatch a command via the captured handler and return the parsed response payload. */
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

describe('list_console_errors: bridge handler', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    // Install capture and clear buffer before each test
    installConsoleErrorCapture();
    clearConsoleErrors();

    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('returns {errors:[], count:0} when buffer is empty', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'list_console_errors') as any;
    expect(result).toBeDefined();
    expect(Array.isArray(result.errors)).toBe(true);
    expect(result.errors).toHaveLength(0);
    expect(result.count).toBe(0);
  });

  it('returns captured errors with message and stack', async () => {
    const err = new Error('xyz-error');
    console.error('xyz', err);

    const result = await dispatchAndGetResult(capturedHandler!, 2, 'list_console_errors') as any;
    expect(Array.isArray(result.errors)).toBe(true);
    expect(result.count).toBeGreaterThanOrEqual(1);

    const entry = result.errors.find((e: any) => e.message.includes('xyz'));
    expect(entry).toBeDefined();
    expect(entry.stack).not.toBeNull();
  });

  it('{clear:true} returns the snapshot and drains the buffer', async () => {
    console.error('drain-me');

    const result = await dispatchAndGetResult(capturedHandler!, 3, 'list_console_errors', { clear: true }) as any;
    expect(result.count).toBeGreaterThanOrEqual(1);
    const entry = result.errors.find((e: any) => e.message.includes('drain-me'));
    expect(entry).toBeDefined();

    // Subsequent call should return empty
    const result2 = await dispatchAndGetResult(capturedHandler!, 4, 'list_console_errors') as any;
    expect(result2.count).toBe(0);
    expect(result2.errors).toHaveLength(0);
  });
});
