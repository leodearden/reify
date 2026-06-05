/**
 * task-4295 SIGNAL capstone: list_elements enumerates chrome surfaces.
 *
 * Dispatches the real list_elements handler (via initDebugBridge harness)
 * over rendered chrome (MenuBar + FileTabs + DiagnosticsPanel) and asserts
 * testId PRESENCE. Visibility is always false in jsdom (rect.width===0) —
 * enumeration (testId in the returned set) is the contract under test.
 */

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
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import type { DebugStores } from '../debug/types';
import { makeViewStateStoreMock } from './debugBridgeTestHelpers';
import { MenuBar } from '../panels/MenuBar';
import { FileTabs } from '../editor/FileTabs';
import { DiagnosticsPanel } from '../panels/DiagnosticsPanel';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';
import { createEditorStore } from '../stores/editorStore';
import type { FileData } from '../types';

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
    viewState: makeViewStateStoreMock(),
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

describe('list_elements — chrome surface enumeration (T0 capstone)', () => {
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
    cleanup();
    delete window.__REIFY_DEBUG__;
  });

  it('enumerates menu-trigger-* and menu-item-open (newly tagged) plus file-tab and diagnostic-row (already tagged)', async () => {
    // 1. Init the bridge — sets window.__REIFY_DEBUG__
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // 2. Render chrome components
    //    MenuBar — open the File menu so item buttons mount in the DOM
    render(() => <MenuBar />);
    fireEvent.click(screen.getByTestId('menu-trigger-file'));

    //    FileTabs — two open files → two file-tab elements
    const editorStore = createEditorStore();
    const f1: FileData = { path: '/proj/a.ri', content: '' };
    const f2: FileData = { path: '/proj/b.ri', content: '' };
    editorStore.openFile(f1);
    editorStore.openFile(f2);
    render(() => <FileTabs store={editorStore} />);

    //    DiagnosticsPanel — open=true, one error row → one diagnostic-row element
    const diag: DiagnosticEntry = {
      file_path: '/proj/a.ri',
      line: 1,
      column: 1,
      end_line: 1,
      end_column: 5,
      severity: 'Error',
      message: 'test error',
      code: null,
      source: 'compile',
    };
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[diag]}
        onClose={() => {}}
        onNavigate={() => {}}
      />
    ));

    // 3. Dispatch list_elements command
    vi.mocked(invoke).mockResolvedValue(undefined);
    await capturedHandler!({ payload: { id: 42, command: 'list_elements', params: {} } });

    // 4. Parse the response
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result) as { elements: Array<{ testId: string }> };
    const testIds = new Set(result.elements.map((e) => e.testId));

    // 5. Assert NEWLY-tagged menu surfaces are present
    expect(testIds.has('menu-trigger-file')).toBe(true);
    expect(testIds.has('menu-trigger-edit')).toBe(true);
    expect(testIds.has('menu-trigger-view')).toBe(true);
    expect(testIds.has('menu-trigger-help')).toBe(true);
    // File menu is open — item buttons are in the DOM
    expect(testIds.has('menu-item-open')).toBe(true);

    // 6. Assert ALREADY-tagged surfaces also enumerate (confirming sweep coverage)
    expect(testIds.has('file-tab')).toBe(true);
    expect(testIds.has('diagnostic-row')).toBe(true);
  });
});
