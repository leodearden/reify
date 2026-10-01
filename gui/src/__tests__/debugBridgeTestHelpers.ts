/**
 * Shared test helpers for the debug-bridge test suite: the store mocks a
 * bridge is initialised with, and the dispatcher that drives a debug request
 * through it. One copy each, so a change to a store shape or to the response
 * envelope is one edit rather than one per test file.
 */
import { vi, expect } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { ViewStateStore } from '../stores/viewStateStore';
import type { DebugStores } from '../debug/types';

/**
 * Returns a ViewStateStore mock with vi.fn() stubs for every method
 * currently reachable from bridge handlers, plus `switchView` as a
 * leading indicator for view-management work.
 *
 * When a new bridge handler starts calling another ViewStateStore method,
 * add its stub here — the spy fires rather than throwing
 * "undefined is not a function" at runtime, closing the latent gap that
 * the `as unknown as ViewStateStore` cast would otherwise hide.
 */
export function makeViewStateStoreMock(): ViewStateStore {
  return {
    resetToDefaultView: vi.fn(),
    switchView: vi.fn().mockReturnValue(false),
  } as unknown as ViewStateStore;
}

/**
 * A minimal DebugStores: idle engine, empty editor and selection. Enough for a
 * debug-bridge test whose handlers drive the DOM rather than a store.
 */
export function makeDebugStores(): DebugStores {
  return {
    engine: {
      state: {
        meshes: {},
        values: {},
        constraints: {},
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
      },
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

export type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

/**
 * Build a `dispatchCmd`: invoke the captured debug-request handler and return
 * the parsed `debug_response` payload. Requires `@tauri-apps/api/core` to be
 * mocked by the calling test file.
 *
 * debugBridge.test.tsx once held 17 byte-identical copies of this body, one per
 * describe block, so a change to the response envelope meant 17 edits and any
 * missed one drifted silently. Takes a THUNK rather than the handler itself
 * because each caller's `capturedHandler` is reassigned by its `beforeEach` —
 * capturing the value here would freeze it at `undefined`.
 *
 * The callers' `beforeEach`/`afterEach` pairs are deliberately NOT folded in:
 * they genuinely differ (some call `initDebugBridge` up front, others per test;
 * some `cleanup()`, others `vi.restoreAllMocks()`), so a shared one would have
 * to be parameterised into something longer than the lines it replaced.
 */
export function makeCmdDispatcher(getHandler: () => DebugRequestHandler | undefined) {
  return async function dispatchCmd(
    id: number,
    command: string,
    params: Record<string, unknown>,
  ) {
    vi.mocked(invoke).mockClear();
    await getHandler()!({ payload: { id, command, params } });
    const responseCall = vi.mocked(invoke).mock.calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  };
}
