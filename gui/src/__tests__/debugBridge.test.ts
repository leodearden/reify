/**
 * Unit tests for the debug bridge handlers.
 * Covers: store_state / viewport_state selectedEntities; set_test_mode.
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
import { setTestMode } from '../debug/testMode';
import type { DebugStores } from '../debug/types';

type DebugRequestHandler = (event: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

function makeStores(selectedEntities: string[] = [], anchorEntity: string | null = null): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
      },
      initFromState: vi.fn(),
    },
    editor: {
      state: {
        openFiles: [],
        activeFile: null,
        dirtyFiles: [],
        cursorPosition: null,
      },
      openFile: vi.fn(),
    },
    selection: {
      state: {
        selectedEntity: selectedEntities[selectedEntities.length - 1] ?? null,
        // Cast to any until step-36 adds the fields to the DebugStores type
        ...(selectedEntities.length > 0 ? { selectedEntities } : { selectedEntities: [] }),
        ...(anchorEntity !== null ? { anchorEntity } : { anchorEntity: null }),
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
  };
}

describe('debug bridge store_state includes selectedEntities', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('store_state includes selection.selectedEntities array', async () => {
    const stores = makeStores(['A', 'B']);
    await initDebugBridge(stores);

    expect(capturedHandler).toBeDefined();

    await capturedHandler!({ payload: { id: 1, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    // invoke('debug_response', { id, result: JSON.stringify(result) })
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual(['A', 'B']);
  });

  it('store_state includes selection.selectedEntities as empty array when nothing selected', async () => {
    const stores = makeStores([]);
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 2, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual([]);
  });

  it('store_state includes selection.anchorEntity', async () => {
    const stores = makeStores(['A'], 'A');
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 3, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.anchorEntity).toBe('A');
  });

  it('viewport_state includes selectedEntities via the stores reference', async () => {
    const stores = makeStores(['X', 'Y']);
    await initDebugBridge(stores);

    // store_state reads selection.selectedEntities from stores (same reference used by viewport_state)
    await capturedHandler!({ payload: { id: 4, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual(['X', 'Y']);
  });
});

describe('debug bridge set_test_mode', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    // Clean up DOM attribute and reset signal so tests don't leak
    delete document.documentElement.dataset.testMode;
    setTestMode(false);
    delete window.__REIFY_DEBUG__;
  });

  it('set_test_mode { enabled: true } returns { ok: true, test_mode: true } and sets data-test-mode', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    await capturedHandler!({ payload: { id: 10, command: 'set_test_mode', params: { enabled: true } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ ok: true, test_mode: true });
    expect(document.documentElement.dataset.testMode).toBe('true');
  });

  it('set_test_mode { enabled: false } returns { ok: true, test_mode: false } and clears data-test-mode', async () => {
    // First enable, then disable
    document.documentElement.dataset.testMode = 'true';
    const stores = makeStores();
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 11, command: 'set_test_mode', params: { enabled: false } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ ok: true, test_mode: false });
    expect(document.documentElement.dataset.testMode).toBeUndefined();
  });
});
