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
import { initDebugBridge, RESOLVE_BY_TESTID_ERRORS } from '../debug/bridge';
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

  it('returns {error} immediately for an invalid state value', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 6, 'wait_for_selector', {
      testId: 'some-el',
      state: 'sideways',
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

// ─── #5891 viewport scoping — step-11 RED → step-12 GREEN ────────────────────
//
// wait_for_selector and wait_for's selector arm share buildSelectorPredicate, so
// one block covers both tools.
//
// These two OBSERVE rather than drive: unlike click_element / focus_element /
// scroll / element_screenshot there is no wrong-target ACTION to disclose, so
// the return shapes stay exactly pollUntil's — {ok:true, waited_ms} and
// {error:'timeout'} — with no viewportId/matchCount echo even on a multi-match.
// Scoping the predicate IS the whole fix here.
describe('wait_for_selector / wait_for: viewport scoping (#5891)', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let root: HTMLElement | null = null;

  /** Give an element a non-zero rect so isElementVisible() reports true under jsdom. */
  function makeVisible(el: Element) {
    (el as HTMLElement).getBoundingClientRect = () => ({
      x: 0, y: 0, width: 100, height: 20,
      top: 0, right: 100, bottom: 20, left: 0,
      toJSON: () => ({}),
    });
  }

  /**
   * `design-main` ALWAYS holds a visible `scoped-el`; `pane-1` holds one only when
   * `inPane1`. That asymmetry is the discriminator for every negative case below:
   * a predicate still falling back to the document-wide lookup would be satisfied
   * by design-main's element no matter which pane was actually asked for.
   */
  function mountPanes(inPane1: boolean) {
    root = document.createElement('div');
    root.innerHTML = `
      <div data-viewport-id="design-main"><div data-testid="scoped-el"></div></div>
      <div data-viewport-id="pane-1">${inPane1 ? '<div data-testid="scoped-el"></div>' : ''}</div>
    `;
    document.body.appendChild(root);
    root.querySelectorAll('[data-testid="scoped-el"]').forEach(makeVisible);
  }

  /** Settle pending microtasks WITHOUT advancing any fake timer. */
  async function flushMicrotasks() {
    for (let i = 0; i < 8; i += 1) await Promise.resolve();
  }

  /**
   * Dispatch, then always drain past the timeout so no dangling promise outlives a
   * failed assertion. `settledBeforePolling` still records whether the handler
   * answered without ever reaching pollUntil's 16 ms tick — the only way to see a
   * fail-fast rejection as distinct from one that burned the timeout budget.
   */
  async function dispatchDrained(id: number, command: string, params: Record<string, unknown>) {
    vi.mocked(invoke).mockClear();
    const dispatchPromise = capturedHandler!({ payload: { id, command, params } });
    await flushMicrotasks();
    const settledBeforePolling = vi.mocked(invoke).mock.calls.some((c) => c[0] === 'debug_response');
    await vi.advanceTimersByTimeAsync(500);
    await dispatchPromise;
    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    return {
      result: JSON.parse((responseCall![1] as any).result) as any,
      settledBeforePolling,
    };
  }

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
    if (root && root.parentNode) document.body.removeChild(root);
    root = null;
    delete window.__REIFY_DEBUG__;
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('(a) scoped wait_for_selector resolves {ok:true} when the element is visible IN THAT PANE', async () => {
    mountPanes(true);

    const { result } = await dispatchDrained(20, 'wait_for_selector', {
      testId: 'scoped-el', state: 'visible', viewportId: 'pane-1', timeout_ms: 100,
    });

    expect(result.ok).toBe(true);
    expect(typeof result.waited_ms).toBe('number');
  });

  it('(b) scoped: an element visible only in ANOTHER pane never satisfies the wait', async () => {
    // THE discriminating case. design-main holds a visible 'scoped-el'; pane-1 does
    // not. A document-wide fallback would resolve {ok:true} off the wrong pane.
    mountPanes(false);

    const { result } = await dispatchDrained(21, 'wait_for_selector', {
      testId: 'scoped-el', state: 'visible', viewportId: 'pane-1', timeout_ms: 100,
    });

    expect(result).toEqual({ error: 'timeout' });
  });

  it('(c) scoped state:"gone": an element still present in ANOTHER pane counts as GONE', async () => {
    // The mirror of (b): scoping has to cut both ways. Unscoped, design-main's
    // still-visible element would keep the 'gone' predicate false until timeout.
    mountPanes(false);

    const { result } = await dispatchDrained(22, 'wait_for_selector', {
      testId: 'scoped-el', state: 'gone', viewportId: 'pane-1', timeout_ms: 100,
    });

    expect(result.ok).toBe(true);
  });

  it('(d) non-string viewportId is rejected BEFORE any polling', async () => {
    mountPanes(true);

    const { result, settledBeforePolling } = await dispatchDrained(23, 'wait_for_selector', {
      testId: 'scoped-el', state: 'visible', viewportId: 5, timeout_ms: 100,
    });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
    // A malformed param must fail fast, not burn the caller's whole timeout
    // budget re-deciding the same rejection on every 16 ms tick.
    expect(settledBeforePolling).toBe(true);
  });

  it('(e) wait_for selector arm: predicate.viewportId scopes it the same way', async () => {
    mountPanes(true);

    const { result } = await dispatchDrained(24, 'wait_for', {
      predicate: { kind: 'selector', testId: 'scoped-el', state: 'visible', viewportId: 'pane-1' },
      timeout_ms: 100,
    });

    expect(result.ok).toBe(true);
  });

  it('(e) wait_for selector arm: an element only in ANOTHER pane never satisfies it', async () => {
    mountPanes(false);

    const { result } = await dispatchDrained(25, 'wait_for', {
      predicate: { kind: 'selector', testId: 'scoped-el', state: 'visible', viewportId: 'pane-1' },
      timeout_ms: 100,
    });

    expect(result).toEqual({ error: 'timeout' });
  });

  it('(e) wait_for selector arm: non-string predicate.viewportId also fails fast', async () => {
    // Asserted separately from (d) because the two tools reach the shared
    // predicate through DIFFERENT call sites — hoisting the validation out of
    // one closure and not the other would leave this arm polling to timeout.
    mountPanes(true);

    const { result, settledBeforePolling } = await dispatchDrained(26, 'wait_for', {
      predicate: { kind: 'selector', testId: 'scoped-el', state: 'visible', viewportId: 5 },
      timeout_ms: 100,
    });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
    expect(settledBeforePolling).toBe(true);
  });

  it('(f) UNSCOPED multi-match keeps the unchanged {ok:true, waited_ms} shape — no pane keys', async () => {
    mountPanes(true); // two matches — a DRIVING tool would report its guess here

    const { result } = await dispatchDrained(27, 'wait_for_selector', {
      testId: 'scoped-el', state: 'visible', timeout_ms: 100,
    });

    expect(result.ok).toBe(true);
    expect(Object.keys(result).sort()).toEqual(['ok', 'waited_ms']);
  });

  it('(f) UNSCOPED timeout keeps the unchanged {error:"timeout"} shape', async () => {
    mountPanes(true);

    const { result } = await dispatchDrained(28, 'wait_for_selector', {
      testId: 'no-such-el', state: 'visible', timeout_ms: 100,
    });

    expect(result).toEqual({ error: 'timeout' });
  });

  // (g) pins the ONE asymmetry a harness author can trip over, called out on both
  // tools' schema descriptions. `notFoundForViewport` folds into `el === null`,
  // so 'gone' is satisfied vacuously by a pane that does not exist — a pane not
  // yet mounted, or an id with a typo in it, is indistinguishable from a real
  // teardown. Kept deliberately rather than hard-failed: a pane torn down WITH
  // its contents is a legitimate way for an element to be gone from it, and
  // requiring the pane to still exist would make "wait for this pane to
  // disappear" un-expressible and turn a correct green into a timeout.
  it('(g) state:"gone" against a pane that does not exist resolves IMMEDIATELY — vacuously gone', async () => {
    mountPanes(true); // design-main and pane-1 exist; 'pane-typo' does not

    const { result, settledBeforePolling } = await dispatchDrained(29, 'wait_for_selector', {
      testId: 'scoped-el', state: 'gone', viewportId: 'pane-typo', timeout_ms: 100,
    });

    expect(result.ok).toBe(true);
    // Not merely "eventually true": the predicate is satisfied on its first
    // evaluation, before pollUntil's first 16 ms tick. That is what makes a
    // typo'd pane id a SILENT instant success rather than a slow one.
    expect(settledBeforePolling).toBe(true);
  });

  it('(g) the same absent pane under state:"visible" instead fails LOUDLY with a timeout', async () => {
    // The contrast that makes (g) safe to keep: a typo is only invisible on the
    // 'gone' arm. Documenting the asymmetry is the whole mitigation, so pin that
    // it IS an asymmetry and not a blanket "absent pane always succeeds".
    mountPanes(true);

    const { result } = await dispatchDrained(30, 'wait_for_selector', {
      testId: 'scoped-el', state: 'visible', viewportId: 'pane-typo', timeout_ms: 100,
    });

    expect(result).toEqual({ error: 'timeout' });
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

  it('returns {error} when selector predicate has no testId', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 16, 'wait_for', {
      predicate: { kind: 'selector', state: 'visible' },
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });

  it('returns {error} immediately for store predicate with unknown root path', async () => {
    // A typo'd root should surface a clear error rather than silently timing out.
    const result = await dispatchAndGetResult(capturedHandler!, 17, 'wait_for', {
      predicate: { kind: 'store', path: 'viewState.something', equals: 'x' },
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });

  it('returns {error} immediately for store predicate with missing equals', async () => {
    // equals is required; omitting it is ambiguous (undefined matches any undefined path).
    const result = await dispatchAndGetResult(capturedHandler!, 18, 'wait_for', {
      predicate: { kind: 'store', path: 'engine.evalStatus.phase' },
    }) as any;
    expect(result.error).toBeDefined();
    expect(typeof result.error).toBe('string');
  });
});
