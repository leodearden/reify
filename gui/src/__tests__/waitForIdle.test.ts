/**
 * Unit tests for the wait_for_idle debug bridge handler.
 * Covers: async dispatch, happy path, phase-transition wait, timeout, default timeout.
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
import type { DebugStores } from '../debug/types';
import type { ViewStateStore } from '../stores/viewStateStore';

type DebugRequestHandler = (event: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

function makeStores(phase: 'idle' | 'evaluating' | 'error' = 'idle'): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase },
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
    },
  };
}

/** Dispatch a command via the captured handler and return the parsed response payload. */
async function dispatchAndGetResult(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  if (!responseCall) return undefined;
  const payload = responseCall[1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

describe('wait_for_idle: bridge dispatch awaits async handler results', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    // Stub requestAnimationFrame to fire callback synchronously
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    delete window.__REIFY_DEBUG__;
  });

  it('wait_for_idle when already idle returns {ok: true} — proving await works', async () => {
    // Starts idle so the handler should resolve immediately (after rAF tick).
    // Without `await handler(params)`, a Promise would serialize as `{}`.
    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    const result = await dispatchAndGetResult(capturedHandler!, 1, 'wait_for_idle', {});
    expect(result).toMatchObject({ ok: true });
    // idle_after_ms must be a number (not a serialized Promise `{}`)
    expect(typeof (result as any).idle_after_ms).toBe('number');
  });
});

describe('wait_for_idle: returns ok after engine becomes idle', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    delete window.__REIFY_DEBUG__;
  });

  it('wait_for_idle returns ok with idle_after_ms after engine becomes idle', async () => {
    // Start with evaluating phase
    const stores = makeStores('evaluating');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // Stub rAF to fire synchronously once called
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });

    // Kick off the dispatch (won't resolve until phase becomes idle)
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({ payload: { id: 2, command: 'wait_for_idle', params: {} } });

    // Advance timers a bit — still evaluating, handler should still be polling
    await vi.advanceTimersByTimeAsync(32);
    // Should not have responded yet
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Transition to idle
    stores.engine.state.evalStatus.phase = 'idle';

    // Advance timers enough for one more polling tick (~16ms) so the while loop observes idle
    await vi.advanceTimersByTimeAsync(32);

    // Now await the dispatch to complete
    await dispatchPromise;

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toMatchObject({ ok: true });
    expect(typeof result.idle_after_ms).toBe('number');
    expect(result.idle_after_ms).toBeGreaterThanOrEqual(0);
  });
});

describe('wait_for_idle: terminal non-idle phases', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    // rAF stub — fires synchronously (only reached on success path)
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    delete window.__REIFY_DEBUG__;
  });

  it('returns {error: "engine_phase", phase: "error"} immediately when phase is "error"', async () => {
    // Start with error phase — the engine failed, not just busy.
    const stores = makeStores('error');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    const result = await dispatchAndGetResult(capturedHandler!, 5, 'wait_for_idle', {});
    expect(result).toEqual({ error: 'engine_phase', phase: 'error' });
  });
});

describe('wait_for_idle: timeout enforcement', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    // rAF stub — fires synchronously (only reached on success path)
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    delete window.__REIFY_DEBUG__;
  });

  it('returns {error: "timeout"} when phase stays non-idle past timeout_ms', async () => {
    const stores = makeStores('evaluating');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    vi.mocked(invoke).mockClear();
    // Dispatch with explicit 100ms timeout — phase stays 'evaluating'
    const dispatchPromise = capturedHandler!({
      payload: { id: 3, command: 'wait_for_idle', params: { timeout_ms: 100 } },
    });

    // Advance past the timeout without changing phase
    await vi.advanceTimersByTimeAsync(200);
    await dispatchPromise;

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ error: 'timeout' });
  });

  it('uses default 30000ms timeout when timeout_ms is omitted', async () => {
    const stores = makeStores('evaluating');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 4, command: 'wait_for_idle', params: {} },
    });

    // Advance to just before the default 30000ms timeout — no response yet
    await vi.advanceTimersByTimeAsync(29999);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Advance past the default timeout
    await vi.advanceTimersByTimeAsync(2000);
    await dispatchPromise;

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ error: 'timeout' });
  });
});
