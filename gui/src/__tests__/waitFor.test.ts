/**
 * Unit tests for wait_for_selector and wait_for bridge commands.
 * Mirrors waitForIdle.test.ts harness: mock listen/invoke/three, capture handler,
 * makeStores helper, useFakeTimers + advanceTimersByTimeAsync for blocking cases.
 *
 * JSDOM constraints:
 * - getBoundingClientRect() returns all-zero rect; stub to width>0 for 'visible' tests.
 * - el.textContent is used for text-equals (jsdom lacks innerText).
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
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
    viewState: {
      resetToDefaultView: vi.fn(),
    },
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

/** Dispatch a command and return the parsed response payload. */
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

// ─── Part A: wait_for_selector ────────────────────────────────────────────────

describe('wait_for_selector: element already visible', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let testEl: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    // Create and append a visible element (stub getBoundingClientRect for jsdom)
    testEl = document.createElement('div');
    testEl.setAttribute('data-testid', 'test-target');
    testEl.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
    document.body.appendChild(testEl);

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    document.body.removeChild(testEl);
    delete window.__REIFY_DEBUG__;
    vi.unstubAllGlobals();
  });

  it('resolves immediately with {ok:true} when element is already visible', async () => {
    // Element is in DOM and getBoundingClientRect returns width>0
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'wait_for_selector', {
      testId: 'test-target',
      state: 'visible',
    }) as any;
    expect(result.ok).toBe(true);
    expect(typeof result.waited_ms).toBe('number');
  });
});

describe('wait_for_selector: appears after delay', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let testEl: HTMLElement | null = null;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    if (testEl && testEl.parentNode) {
      document.body.removeChild(testEl);
    }
    testEl = null;
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('resolves {ok:true} once element appears with width>0', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 2, command: 'wait_for_selector', params: { testId: 'lazy-el', state: 'visible' } },
    });

    // Advance — element not yet in DOM, no response expected
    await vi.advanceTimersByTimeAsync(32);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Append element with stubbed rect
    testEl = document.createElement('div');
    testEl.setAttribute('data-testid', 'lazy-el');
    testEl.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
    document.body.appendChild(testEl);

    // Advance one more 16ms tick — poll should observe the new element
    await vi.advanceTimersByTimeAsync(32);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.ok).toBe(true);
  });
});

describe('wait_for_selector: state gone', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let testEl: HTMLElement;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    // Start with element present
    testEl = document.createElement('div');
    testEl.setAttribute('data-testid', 'removable');
    testEl.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
    document.body.appendChild(testEl);

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    if (testEl.parentNode) {
      document.body.removeChild(testEl);
    }
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('resolves {ok:true} once element is removed from DOM', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 3, command: 'wait_for_selector', params: { testId: 'removable', state: 'gone' } },
    });

    // Element is visible — gone predicate not satisfied
    await vi.advanceTimersByTimeAsync(32);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Remove the element
    document.body.removeChild(testEl);

    // Advance for one more poll tick
    await vi.advanceTimersByTimeAsync(32);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.ok).toBe(true);
  });
});

describe('wait_for_selector: text-equals predicate', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let testEl: HTMLElement;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    testEl = document.createElement('div');
    testEl.setAttribute('data-testid', 'text-el');
    testEl.textContent = 'Loading';
    testEl.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
    document.body.appendChild(testEl);

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    if (testEl.parentNode) {
      document.body.removeChild(testEl);
    }
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('resolves {ok:true} once textContent matches the text param (using textContent, not innerText)', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 4, command: 'wait_for_selector', params: { testId: 'text-el', state: 'visible', text: 'Ready' } },
    });

    // Text is 'Loading' — not matched yet
    await vi.advanceTimersByTimeAsync(32);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Update textContent to 'Ready'
    testEl.textContent = 'Ready';

    // Advance for one more poll tick
    await vi.advanceTimersByTimeAsync(32);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.ok).toBe(true);
  });
});

describe('wait_for_selector: timeout', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('returns {error:"timeout"} when predicate never satisfied', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 5, command: 'wait_for_selector', params: { testId: 'never-exists', state: 'visible', timeout_ms: 100 } },
    });

    // Advance past the 100ms timeout
    await vi.advanceTimersByTimeAsync(200);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.error).toBe('timeout');
  });
});

describe('wait_for_selector: validation errors', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.unstubAllGlobals();
  });

  it('returns {error} immediately when testId is missing', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 6, 'wait_for_selector', {
      state: 'visible',
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });
});

describe('wait_for_selector: default timeout', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('uses default 5000ms when timeout_ms is omitted', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: { id: 7, command: 'wait_for_selector', params: { testId: 'never-el', state: 'visible' } },
    });

    // Just before the 5000ms default — no response yet
    await vi.advanceTimersByTimeAsync(4999);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Past the default timeout
    await vi.advanceTimersByTimeAsync(2000);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.error).toBe('timeout');
  });
});

// ─── Part B: wait_for ────────────────────────────────────────────────────────

describe('wait_for: selector kind', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let testEl: HTMLElement | null = null;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    if (testEl && testEl.parentNode) {
      document.body.removeChild(testEl);
    }
    testEl = null;
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('selector predicate: appears-after-delay resolves {ok:true}', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: {
        id: 10,
        command: 'wait_for',
        params: { predicate: { kind: 'selector', testId: 'wait-for-el', state: 'visible' } },
      },
    });

    // Element not yet present
    await vi.advanceTimersByTimeAsync(32);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Append element with stubbed rect
    testEl = document.createElement('div');
    testEl.setAttribute('data-testid', 'wait-for-el');
    testEl.getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
    document.body.appendChild(testEl);

    await vi.advanceTimersByTimeAsync(32);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.ok).toBe(true);
  });
});

describe('wait_for: store kind', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let stores: DebugStores;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    stores = makeStores('evaluating');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('store predicate: resolves {ok:true} once dotted-path value matches', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: {
        id: 11,
        command: 'wait_for',
        params: {
          predicate: { kind: 'store', path: 'engine.evalStatus.phase', equals: 'idle' },
          timeout_ms: 5000,
        },
      },
    });

    // Phase is 'evaluating' — not matched
    await vi.advanceTimersByTimeAsync(32);
    expect(vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response')).toBeUndefined();

    // Transition store to idle
    stores.engine.state.evalStatus.phase = 'idle';

    // Advance one poll tick
    await vi.advanceTimersByTimeAsync(32);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.ok).toBe(true);
  });

  it('store predicate: times out when value never matches', async () => {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({
      payload: {
        id: 12,
        command: 'wait_for',
        params: {
          predicate: { kind: 'store', path: 'engine.evalStatus.phase', equals: 'idle' },
          timeout_ms: 100,
        },
      },
    });

    // Phase stays 'evaluating'
    await vi.advanceTimersByTimeAsync(200);
    await dispatchPromise;

    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as any).result);
    expect(result.error).toBe('timeout');
  });
});

describe('wait_for: validation errors', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores('idle');
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    vi.unstubAllGlobals();
  });

  it('returns {error} for unknown predicate kind', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 13, 'wait_for', {
      predicate: { kind: 'nope' },
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });

  it('returns {error} when predicate is not an object', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 14, 'wait_for', {
      predicate: 'string-not-object',
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });

  it('returns {error} when predicate is missing', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 15, 'wait_for', {}) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });
});
