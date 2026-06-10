/**
 * Shared harness utilities for the IDE affordances e2e integration test (task 4211 λ).
 *
 * Provides:
 *   - FIXTURE .ri constant with two structures each declaring a `width` param
 *     (PRD §Boundary-test row 1: rename scope correctness observable)
 *   - Contract-faithful LSP mock router keyed by LSP method + request recorder
 *   - dispatchCmd helper (mirrors debugContract.test.ts dispatch pattern)
 *   - makeBaseStores() factory (real editorStore + selectionStore, mocked viewState)
 *
 * vi.mock() calls live in the test file (they must be hoisted to file-top level).
 * This module is test-only (lives under __tests__/) and freely imports vi.
 */

import { vi } from 'vitest';
import { createRoot, createSignal } from 'solid-js';
import { render } from '@solidjs/testing-library';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import * as bridge from '../bridge';
import type { DebugStores } from '../debug/types';
import { createEditorStore } from '../stores/editorStore';
import { createSelectionStore } from '../stores/selectionStore';
import { createEngineStore } from '../stores/engineStore';
import { createLayoutStore } from '../stores/layoutStore';
import { makeViewStateStoreMock } from './debugBridgeTestHelpers';
import { initDebugBridge } from '../debug/bridge';
import { Editor } from '../editor/Editor';
import { FindUsesPanel } from '../panels/FindUsesPanel';
import type { ReferenceResult } from '../editor/references';

// ── FIXTURE ───────────────────────────────────────────────────────────────────

/**
 * Two-structure .ri fixture where BOTH PartA and PartB declare a `width` param.
 * PRD §Boundary-test row 1: rename on PartA.width rewrites only PartA occurrences;
 * PartB.width must remain untouched.
 *
 * Line layout (0-based):
 *   0  structure PartA {
 *   1    param width = 10.0     ← PartA.width decl  (char 8..13)
 *   2    param height = 20.0
 *   3  (empty)
 *   4    body box {
 *   5      x_size = width       ← PartA.width use   (char 13..18)
 *   6      y_size = height
 *   7    }
 *   8  }
 *   9  (empty)
 *  10  structure PartB {
 *  11    param width = 5.0      ← PartB.width decl  (char 8..13)
 *  12    param depth = 15.0
 *  13  (empty)
 *  14    body cylinder {
 *  15      radius = width / 2   ← PartB.width use   (char 13..18)
 *  16      length = depth
 *  17    }
 *  18  }
 */
export const FIXTURE = `structure PartA {
  param width = 10.0
  param height = 20.0

  body box {
    x_size = width
    y_size = height
  }
}

structure PartB {
  param width = 5.0
  param depth = 15.0

  body cylinder {
    radius = width / 2
    length = depth
  }
}
`;

export const FIXTURE_PATH = '/test/fixture.ri';

// ── LSP mock router ───────────────────────────────────────────────────────────

/** A single recorded LSP request (method + parsed params). */
export interface LspCall {
  method: string;
  params: unknown;
}

/**
 * Contract-faithful LSP response router keyed by method.
 * Returns JSON strings matching lspClient.ts response shapes.
 *
 * Callers should populate `recorder` to track all lsp_request invocations:
 *   vi.mocked(invoke).mockImplementation(async (cmd, args) => {
 *     if (cmd === 'lsp_request') {
 *       const { method, params } = args as { method: string; params: string };
 *       recorder.push({ method, params: JSON.parse(params) });
 *       return createLspResponseFor(method, params);
 *     }
 *     return undefined;
 *   });
 */
export function createLspResponseFor(method: string, paramsJson: string): string {
  switch (method) {
    case 'initialize':
      return JSON.stringify({ capabilities: {} });

    case 'initialized':
    case 'textDocument/didOpen':
    case 'textDocument/didChange':
    case 'textDocument/didClose':
      return JSON.stringify(null);

    case 'textDocument/references': {
      // Return PartA's two width occurrences (declaration + use).
      const locs = [
        {
          uri: `file://${FIXTURE_PATH}`,
          range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
        },
        {
          uri: `file://${FIXTURE_PATH}`,
          range: { start: { line: 5, character: 13 }, end: { line: 5, character: 18 } },
        },
      ];
      return JSON.stringify(locs);
    }

    case 'textDocument/prepareRename': {
      // Token range of `width` at its declaration site (line 1).
      return JSON.stringify({
        range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
        placeholder: 'width',
      });
    }

    case 'textDocument/rename': {
      // WorkspaceEdit: ONLY rewrite PartA's occurrences (lines 1 and 5).
      // PartB's width (lines 11 and 15) is intentionally absent — scope test.
      let newName = 'RENAMED';
      try {
        const p = JSON.parse(paramsJson) as { newName?: string };
        if (p.newName) newName = p.newName;
      } catch {
        // ignore parse failure; use default
      }
      return JSON.stringify({
        changes: {
          [`file://${FIXTURE_PATH}`]: [
            {
              range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
              newText: newName,
            },
            {
              range: { start: { line: 5, character: 13 }, end: { line: 5, character: 18 } },
              newText: newName,
            },
          ],
        },
      });
    }

    case 'textDocument/documentHighlight': {
      return JSON.stringify([
        { range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } }, kind: 1 },
      ]);
    }

    case 'textDocument/documentSymbol': {
      return JSON.stringify([
        {
          name: 'PartA',
          kind: 5,
          range: { start: { line: 0, character: 0 }, end: { line: 8, character: 1 } },
          selectionRange: { start: { line: 0, character: 7 }, end: { line: 0, character: 12 } },
          children: [],
        },
        {
          name: 'PartB',
          kind: 5,
          range: { start: { line: 10, character: 0 }, end: { line: 18, character: 1 } },
          selectionRange: { start: { line: 10, character: 7 }, end: { line: 10, character: 12 } },
          children: [],
        },
      ]);
    }

    case 'textDocument/hover': {
      return JSON.stringify({
        contents: { kind: 'markdown', value: '**width**: float = 10.0' },
        range: { start: { line: 1, character: 8 }, end: { line: 1, character: 13 } },
      });
    }

    case 'textDocument/completion': {
      return JSON.stringify([]);
    }

    case 'textDocument/definition': {
      return JSON.stringify(null);
    }

    default:
      return JSON.stringify(null);
  }
}

// ── Affordance component rendering ───────────────────────────────────────────

/**
 * Renders the real Editor component with the harness editorStore.
 *
 * Must be called AFTER setupBridgeHarness() so that window.__REIFY_DEBUG__ is
 * already set; Editor's onMount then immediately registers ctx.editorView.
 *
 * Spies on bridge.updateSource to avoid real Tauri IPC on the edit debounce.
 * Returns the render result (for cleanup via @solidjs/testing-library cleanup()).
 */
export function renderEditorInHarness(
  harness: HarnessSetup,
): ReturnType<typeof render> {
  // Prevent real Tauri IPC when the 300ms edit debounce fires after tests.
  vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
  vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);

  return render(() => (
    <Editor store={harness.editorStore} />
  ));
}

/**
 * Renders the real Editor + FindUsesPanel with the App-equivalent onShowReferences
 * wiring (step-4 GREEN). Returns the render result and signal accessors so tests
 * can inspect the panel state.
 *
 * Editor's onShowReferences callback feeds the results signal → FindUsesPanel
 * re-renders reactively with the matched find-use-row elements.
 *
 * Must be called AFTER setupBridgeHarness() so window.__REIFY_DEBUG__ is set.
 */
export function renderEditorWithFindUsesPanel(
  harness: HarnessSetup,
): {
  renderResult: ReturnType<typeof render>;
  findUsesOpen: () => boolean;
  findUsesResults: () => ReferenceResult[];
  setFindUsesOpen: (v: boolean) => void;
} {
  vi.spyOn(bridge, 'updateSource').mockResolvedValue(undefined as any);
  vi.spyOn(bridge, 'saveFile').mockResolvedValue(undefined);

  const [findUsesOpen, setFindUsesOpen] = createSignal(false);
  const [findUsesResults, setFindUsesResults] = createSignal<ReferenceResult[]>([]);

  const renderResult = render(() => (
    <div>
      <Editor
        store={harness.editorStore}
        onShowReferences={(results) => {
          setFindUsesResults(() => results);
          setFindUsesOpen(true);
        }}
      />
      <FindUsesPanel
        open={findUsesOpen()}
        results={findUsesResults()}
        onClose={() => setFindUsesOpen(false)}
        onNavigate={() => {}}
      />
    </div>
  ));

  return { renderResult, findUsesOpen, findUsesResults, setFindUsesOpen };
}

// ── dispatchCmd helper ────────────────────────────────────────────────────────

export type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

/**
 * Dispatch a command through the real debug bridge and return the parsed response.
 *
 * Mirrors dispatchCmd from debugContract.test.ts:100-114.
 * Clears the invoke mock before dispatching, then reads back the debug_response call.
 */
export async function dispatchCmd(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  if (!responseCall) throw new Error(`No debug_response call for command: ${command}`);
  const payload = responseCall[1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

// ── Harness setup ─────────────────────────────────────────────────────────────

/** Full harness context returned by setupBridgeHarness(). */
export interface HarnessSetup {
  /** Captured debug-request handler — pass to dispatchCmd / makeDispatch. */
  handler: DebugRequestHandler;
  /** DebugStores for inspecting bridge state. */
  stores: DebugStores;
  /**
   * Full editorStore (superset of DebugStores['editor']).
   * Pass to Editor component so it has access to markDirty, canSave, etc.
   */
  editorStore: ReturnType<typeof createEditorStore>;
  /**
   * Full selectionStore (superset of DebugStores['selection']).
   * Pass to DesignTree's onHover/hoveredEntity props.
   */
  selectionStore: ReturnType<typeof createSelectionStore>;
  /** Recorder of all invoke('lsp_request', ...) calls made after bridge init. */
  lspCalls: LspCall[];
  /** Dispose the reactive root created for the stores. */
  dispose: () => void;
}

/**
 * Initializes the debug bridge with real stores and sets up the invoke mock to route
 * lsp_request calls through the contract-faithful router.
 *
 * Call BEFORE render() so that initDebugBridge sets window.__REIFY_DEBUG__ and
 * component onMount hooks register into ctx (editorView, designTree, etc.).
 */
export async function setupBridgeHarness(): Promise<HarnessSetup> {
  const lspCalls: LspCall[] = [];

  // Route invoke calls: lsp_request → router; others → undefined (debug_response, update_selection, etc.)
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'lsp_request') {
      const { method, params: paramsJson } = args as { method: string; params: string };
      lspCalls.push({ method, params: JSON.parse(paramsJson) });
      return createLspResponseFor(method, paramsJson);
    }
    // debug_response, update_selection, update_source, etc. — resolve to undefined
    return undefined;
  });

  // Capture the debug-request handler when the bridge initializes.
  let capturedHandler: DebugRequestHandler | undefined;
  vi.mocked(listen).mockImplementation(async (event: string, handler: unknown) => {
    if (event === 'debug-request') {
      capturedHandler = handler as DebugRequestHandler;
    }
    return () => {};
  });

  // Create stores inside a reactive root so createEffect (selectionStore) has an owner.
  let editorStore!: ReturnType<typeof createEditorStore>;
  let selectionStore!: ReturnType<typeof createSelectionStore>;
  let stores!: DebugStores;
  let dispose!: () => void;
  createRoot((d) => {
    dispose = d;
    editorStore = createEditorStore();
    selectionStore = createSelectionStore();
    const engineStore = createEngineStore();
    const layoutStore = createLayoutStore();
    const viewState = makeViewStateStoreMock();
    stores = {
      engine: engineStore as DebugStores['engine'],
      editor: editorStore as DebugStores['editor'],
      selection: selectionStore as DebugStores['selection'],
      claude: { state: { messages: [], sessionStatus: 'idle', currentMessageId: null } },
      viewState,
      layout: layoutStore as DebugStores['layout'],
    };
  });

  await initDebugBridge(stores);

  if (!capturedHandler) {
    throw new Error('initDebugBridge did not register a debug-request handler');
  }

  return { handler: capturedHandler, stores, editorStore, selectionStore, lspCalls, dispose };
}

/**
 * Shorthand: dispatch a command using the given handler, auto-incrementing id.
 * Returns a bound dispatch function for the given handler.
 */
export function makeDispatch(handler: DebugRequestHandler) {
  let idCounter = 1;
  return async function dispatch(
    command: string,
    params: Record<string, unknown> = {},
  ): Promise<unknown> {
    return dispatchCmd(handler, idCounter++, command, params);
  };
}
