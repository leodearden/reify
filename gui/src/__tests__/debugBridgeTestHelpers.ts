/**
 * Shared test helpers for the debug-bridge test suite.
 * Centralises the ViewStateStore mock so that adding a newly-reachable
 * bridge method requires updating one place instead of three test files.
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
 * mocked by the calling test file. Takes a THUNK because the caller's captured
 * handler is reassigned per test.
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
