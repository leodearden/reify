import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup, within } from '@solidjs/testing-library';
import type { GuiState } from '../types';
import {
  EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG,
  SAVE_CONFLICT_RELOAD_LABEL,
  SAVE_CONFLICT_OVERWRITE_LABEL,
} from '../editor/messages';
import { flushMacrotasks, deferred, withSuppressedRejections, withSuppressedRejectionsAndErrorSpy } from './test-utils';

// Mock Tauri APIs before any component imports
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

// Capture Viewport props for navigation tests
let capturedViewportProps: any = {};
let capturedDualViewportProps: any = {};
const mockViewportFitToView = vi.fn();
const mockFlyToEntity = vi.fn();
vi.mock('../viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportProps = props;
    // Invoke flyToEntityRef with a trackable mock function if provided
    if (props.flyToEntityRef) {
      props.flyToEntityRef(mockFlyToEntity);
    }
    // Invoke fitToViewRef with a trackable mock function if provided
    if (props.fitToViewRef) {
      props.fitToViewRef(mockViewportFitToView);
    }
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'viewport-container');
    el.textContent = 'Viewport Mock';
    return el;
  },
  DualViewport: (props: any) => {
    capturedDualViewportProps = props;
    // Invoke flyToEntityRef with a trackable mock function if provided
    if (props.flyToEntityRef) {
      props.flyToEntityRef(mockFlyToEntity);
    }
    // Invoke fitToViewRef with a trackable mock function if provided
    if (props.fitToViewRef) {
      props.fitToViewRef(mockViewportFitToView);
    }
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'dual-viewport');
    el.textContent = 'DualViewport Mock';
    return el;
  },
}));

// Mock Editor (requires CodeMirror DOM APIs) — capture store, onOpen, and scrollToLocation for tests
let capturedEditorStore: any = null;
let capturedEditorOnOpen: (() => void) | undefined = undefined;
let capturedEditorScrollToLocation: (() => any) | undefined = undefined;
vi.mock('../editor/Editor', () => ({
  Editor: (props: any) => {
    capturedEditorStore = props.store;
    capturedEditorOnOpen = props.onOpen;
    capturedEditorScrollToLocation = props.scrollToLocation;
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'editor-container');
    el.textContent = 'Editor Mock';
    return el;
  },
}));

// Mock FileTabs
vi.mock('../editor/FileTabs', () => ({
  FileTabs: (_props: any) => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'file-tabs');
    el.textContent = 'FileTabs Mock';
    return el;
  },
}));

// Mock bridge functions
const emptyState: GuiState = { meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] };
vi.mock('../bridge', () => ({
  getInitialState: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] }),
  getEntityTree: vi.fn().mockResolvedValue([]),
  setParameter: vi.fn().mockResolvedValue(undefined),
  exportGeometry: vi.fn().mockResolvedValue(undefined),
  pickSavePath: vi.fn().mockResolvedValue('/user/chosen/path.step'),
  pickOpenPath: vi.fn().mockResolvedValue(null),
  updateSource: vi.fn().mockResolvedValue(undefined),
  saveFile: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockResolvedValue({ path: '', content: '' }),
  openFileEngine: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] }),
  getSourceLocation: vi.fn().mockResolvedValue({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 }),
  focusEntity: vi.fn().mockResolvedValue(undefined),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
  onValueUpdate: vi.fn().mockResolvedValue(() => {}),
  onConstraintUpdate: vi.fn().mockResolvedValue(() => {}),
  onEvaluationStatus: vi.fn().mockResolvedValue(() => {}),
  onMeshRemoved: vi.fn().mockResolvedValue(() => {}),
  onValueRemoved: vi.fn().mockResolvedValue(() => {}),
  onConstraintRemoved: vi.fn().mockResolvedValue(() => {}),
  onTessellationDiagnostics: vi.fn().mockResolvedValue(() => {}),
  onCompileDiagnostics: vi.fn().mockResolvedValue(() => {}),
  onFileChanged: vi.fn().mockResolvedValue(() => {}),
  onFileRemoved: vi.fn().mockResolvedValue(() => {}),
  onSerializationError: vi.fn().mockResolvedValue(() => {}),
  onFocusEntity: vi.fn().mockResolvedValue(() => {}),
  onNavigateToSource: vi.fn().mockResolvedValue(() => {}),
  claudeSendMessage: vi.fn().mockResolvedValue(undefined),
  claudeAbort: vi.fn().mockResolvedValue(undefined),
  claudeClearSession: vi.fn().mockResolvedValue(undefined),
  subscribeToClaudeEvents: vi.fn().mockResolvedValue(() => {}),
  isDebugEnabled: vi.fn().mockResolvedValue(false),
  getKernelStatus: vi.fn().mockResolvedValue({ available: true, message: null }),
  onKernelStatus: vi.fn().mockResolvedValue(() => {}),
  getContainingDefinition: vi.fn().mockResolvedValue(null),
  getEntityAtSourceLocation: vi.fn().mockResolvedValue(null),
  getDefPreview: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] }),
  readViewSidecar: vi.fn().mockResolvedValue(null),
  writeViewSidecar: vi.fn().mockResolvedValue(undefined),
  getMechanismDescriptors: vi.fn().mockResolvedValue([]),
  subscribeToSidecarCrashed: vi.fn().mockResolvedValue(() => {}),
  onAutoResolveStart: vi.fn().mockResolvedValue(() => {}),
  onAutoResolveIteration: vi.fn().mockResolvedValue(() => {}),
  onAutoResolveComplete: vi.fn().mockResolvedValue(() => {}),
  onSolverProgress: vi.fn().mockResolvedValue(() => {}),
  cancelSolve: vi.fn().mockResolvedValue(undefined),
}));

// Mock persistence modules so App.tsx's persistence calls can be intercepted.
vi.mock('../stores/sidecarPersistence', () => ({
  loadSidecar: vi.fn().mockResolvedValue(null),
  saveSidecar: vi.fn().mockResolvedValue(undefined),
}));

// Partial mock: keep createDebouncedSaver + saveViewPersistence real so debounce
// behaviour (step-31) can be tested via localStorage and vi.useFakeTimers.
vi.mock('../stores/viewPersistence', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../stores/viewPersistence')>();
  return {
    ...actual,
    loadViewPersistence: vi.fn().mockReturnValue(null),
  };
});

import App, { NEW_FILE_TEMPLATE } from '../App';
import * as bridge from '../bridge';
import { STORAGE_KEY } from '../hooks/useLayoutPersistence';
import * as sidecarPersistence from '../stores/sidecarPersistence';
import * as viewPersistence from '../stores/viewPersistence';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';

beforeEach(() => {
  vi.clearAllMocks();
  capturedViewportProps = {};
  capturedDualViewportProps = {};
  mockViewportFitToView.mockClear();
  localStorage.clear();
  capturedEditorStore = null;
  capturedEditorOnOpen = undefined;
  capturedEditorScrollToLocation = undefined;
  mockFlyToEntity.mockClear();
  // Reset bridge mocks to defaults (clearAllMocks only clears call history, not implementations)
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });
  vi.mocked(bridge.getEntityTree).mockResolvedValue([]);
  vi.mocked(bridge.onMeshUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(() => {});
  vi.mocked(bridge.onMeshRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onTessellationDiagnostics).mockResolvedValue(() => {});
  vi.mocked((bridge as any).onCompileDiagnostics).mockResolvedValue(() => {});
  vi.mocked(bridge.onFileChanged).mockResolvedValue(() => {});
  vi.mocked((bridge as any).onFileRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onSerializationError).mockResolvedValue(() => {});
  vi.mocked(bridge.onFocusEntity).mockResolvedValue(() => {});
  vi.mocked(bridge.onNavigateToSource).mockResolvedValue(() => {});
  vi.mocked(bridge.subscribeToClaudeEvents).mockResolvedValue(() => {});
  vi.mocked((bridge as any).subscribeToSidecarCrashed).mockResolvedValue(() => {});
  vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/path.step');
  // Persistence module mocks
  vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue(null);
  vi.mocked(sidecarPersistence.saveSidecar).mockResolvedValue(undefined);
  vi.mocked(viewPersistence.loadViewPersistence).mockReturnValue(null);
  vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([]);
  vi.mocked((bridge as any).onSolverProgress).mockResolvedValue(() => {});
  vi.mocked((bridge as any).cancelSolve).mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

/** Helper: render App and wait for init to complete (ready state). */
async function renderAndWaitForReady() {
  const result = render(() => <App />);
  await waitFor(() => {
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });
  return result;
}

describe('App layout wiring', () => {
  it('renders app-layout container', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });

  it('renders MenuBar above Toolbar', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('menu-bar')).toBeTruthy();
  });

  it('renders Toolbar at top', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders StatusBar at bottom', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('renders DualViewport', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('dual-viewport')).toBeTruthy();
  });

  it('renders Editor', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('editor-container')).toBeTruthy();
  });

  it('renders PropertyEditor', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('property-editor')).toBeTruthy();
  });

  it('renders ConstraintPanel', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
  });

  it('renders Toolbar before StatusBar in DOM order', async () => {
    await renderAndWaitForReady();
    const toolbar = screen.getByTestId('toolbar');
    const statusBar = screen.getByTestId('status-bar');
    // Toolbar should come before StatusBar in document order
    const comparison = toolbar.compareDocumentPosition(statusBar);
    expect(comparison & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('App resizable splitters', () => {
  it('has a vertical splitter between editor and viewport columns', async () => {
    await renderAndWaitForReady();
    const splitter = screen.getByTestId('splitter-left');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('has a vertical splitter between viewport and side panel columns', async () => {
    await renderAndWaitForReady();
    const splitter = screen.getByTestId('splitter-right');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('dragging left splitter updates main grid columns', async () => {
    await renderAndWaitForReady();
    const splitter = screen.getByTestId('splitter-left');
    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();

    // Get initial grid-template-columns
    const initialColumns = main.style.gridTemplateColumns;

    // Drag right by 50px
    fireEvent.mouseDown(splitter, { clientX: 300, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 350, clientY: 200 });
    fireEvent.mouseUp(document);

    // Grid template columns should have changed
    const updatedColumns = main.style.gridTemplateColumns;
    expect(updatedColumns).not.toBe(initialColumns);
    // Should contain 350px (300 + 50) for editor panel width
    expect(updatedColumns).toContain('350px');
  });
});

describe('App initial state loading', () => {
  it('calls getInitialState on mount and populates store values into PropertyEditor', async () => {
    const testState: GuiState = {
      meshes: [],
      values: [
        {
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
          kind: 'parameter',
          freshness: 'final',
        },
      ],
      constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    };

    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);

    render(() => <App />);

    // Wait for async getInitialState to resolve and populate the store
    await waitFor(() => {
      // PropertyEditor should show the value from the initial state
      expect(screen.getByText('width')).toBeTruthy();
    });

    expect(bridge.getInitialState).toHaveBeenCalledOnce();
  });
});

describe('App side panel vertical splitter', () => {
  it('has a horizontal splitter between PropertyEditor and ConstraintPanel in the side panel', async () => {
    await renderAndWaitForReady();
    const sidePanel = screen.getByTestId('side-panel');
    const splitter = sidePanel.querySelector('[data-testid="splitter-side"]');
    expect(splitter).toBeTruthy();
    expect((splitter as HTMLElement).dataset.orientation).toBe('horizontal');
  });

  it('PropertyEditor appears before splitter which appears before ConstraintPanel', async () => {
    await renderAndWaitForReady();
    const sidePanel = screen.getByTestId('side-panel');
    const propEditor = screen.getByTestId('property-editor');
    const constraintPanel = screen.getByTestId('constraint-panel');
    const splitter = sidePanel.querySelector('[data-testid="splitter-side"]') as HTMLElement;

    expect(splitter).toBeTruthy();
    // PropertyEditor before splitter
    const propVsSplitter = propEditor.compareDocumentPosition(splitter);
    expect(propVsSplitter & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Splitter before ConstraintPanel
    const splitterVsConstraint = splitter.compareDocumentPosition(constraintPanel);
    expect(splitterVsConstraint & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('dragging the side panel splitter changes the top/bottom split', async () => {
    await renderAndWaitForReady();
    const sidePanel = screen.getByTestId('side-panel');
    const splitter = sidePanel.querySelector('[data-testid="splitter-side"]') as HTMLElement;
    expect(splitter).toBeTruthy();

    // Get initial grid-template-rows or flex-basis
    const initialStyle = sidePanel.style.gridTemplateRows;

    fireEvent.mouseDown(splitter, { clientX: 500, clientY: 300 });
    fireEvent.mouseMove(document, { clientX: 500, clientY: 350 });
    fireEvent.mouseUp(document);

    // Style should have changed after drag
    const updatedStyle = sidePanel.style.gridTemplateRows;
    expect(updatedStyle).not.toBe(initialStyle);
  });
});

describe('App dynamic window title', () => {
  it('sets document.title to "Reify" when no file is open', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    render(() => <App />);

    await waitFor(() => {
      expect(document.title).toBe('Reify');
    });
  });

  it('sets document.title to "{basename} - Reify" when a file is open and idle', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [],
      values: [],
      constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    render(() => <App />);

    await waitFor(() => {
      expect(document.title).toBe('bracket.ri - Reify');
    });
  });

  it('includes evaluation phase in title during evaluation', async () => {
    // Mock onEvaluationStatus to capture the callback so we can trigger it
    let evalStatusCallback: ((status: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });

    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [],
      values: [],
      constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    render(() => <App />);

    // Wait for initial state to load
    await waitFor(() => {
      expect(document.title).toBe('bracket.ri - Reify');
    });

    // Simulate evaluation status change
    evalStatusCallback!({ phase: 'evaluating' });

    await waitFor(() => {
      expect(document.title).toBe('bracket.ri [evaluating] - Reify');
    });
  });
});

describe('App async mount/cleanup race conditions', () => {
  it('does not leak event listeners when unmounted before subscribeToEvents resolves', async () => {
    // Create tracked unlisten functions for all bridge event listeners
    const meshUnlisten = vi.fn();
    const valueUnlisten = vi.fn();
    const constraintUnlisten = vi.fn();
    const evalUnlisten = vi.fn();
    const meshRemovedUnlisten = vi.fn();
    const valueRemovedUnlisten = vi.fn();
    const constraintRemovedUnlisten = vi.fn();

    // Make onMeshUpdate return a deferred promise (delays subscribeToEvents completion)
    const { promise: meshListenPromise, resolve: resolveMeshListen } = deferred<() => void>();
    vi.mocked(bridge.onMeshUpdate).mockReturnValue(meshListenPromise);

    // All other event listeners resolve immediately with tracked unlistens
    vi.mocked(bridge.onValueUpdate).mockResolvedValue(valueUnlisten);
    vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(constraintUnlisten);
    vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(evalUnlisten);
    vi.mocked(bridge.onMeshRemoved).mockResolvedValue(meshRemovedUnlisten);
    vi.mocked(bridge.onValueRemoved).mockResolvedValue(valueRemovedUnlisten);
    vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(constraintRemovedUnlisten);

    const { unmount } = render(() => <App />);

    // Wait for getInitialState to resolve and subscribeToEvents to start
    await flushMacrotasks();

    // Unmount while subscribeToEvents is still pending (waiting for deferred onMeshUpdate)
    unmount();

    // Resolve the deferred onMeshUpdate — subscribeToEvents will now complete
    resolveMeshListen(meshUnlisten);

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // After fix: the alive guard calls the composite unsub immediately,
    // which calls all individual unlisten functions.
    // With current code: unsub is assigned but never called → listeners LEAK
    expect(meshUnlisten).toHaveBeenCalled();
    expect(valueUnlisten).toHaveBeenCalled();
    expect(constraintUnlisten).toHaveBeenCalled();
    expect(evalUnlisten).toHaveBeenCalled();
    expect(meshRemovedUnlisten).toHaveBeenCalled();
    expect(valueRemovedUnlisten).toHaveBeenCalled();
    expect(constraintRemovedUnlisten).toHaveBeenCalled();
  });

  it('does not call initFromState on dead component when unmounted before getInitialState resolves', async () => {
    // Create deferred promise for getInitialState
    const { promise: getStatePromise, resolve: resolveGetState } = deferred<GuiState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(getStatePromise);

    const { unmount } = render(() => <App />);

    // Unmount while getInitialState is still pending
    unmount();

    // Resolve getInitialState with data (values + files)
    resolveGetState({
      meshes: [],
      values: [{
        cell_id: 'c1',
        name: 'testval',
        value: '42',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'Test.testval',
        kind: 'parameter',
        freshness: 'final',
      }],
      constraints: [],
      files: [{ path: '/test.ri', content: '' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // After fix: alive guard returns before reaching subscribeToEvents
    // With current code: initFromState runs, then subscribeToEvents runs → onMeshUpdate called
    expect(bridge.onMeshUpdate).not.toHaveBeenCalled();
  });

  it('unmount calls claudeEventUnsub cleanup function', async () => {
    const claudeUnsub = vi.fn();
    vi.mocked(bridge.subscribeToClaudeEvents).mockResolvedValueOnce(claudeUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Claude unsub should not have been called while component is alive
    expect(claudeUnsub).not.toHaveBeenCalled();

    // Unmount — onCleanup should call claudeEventUnsub?.()
    unmount();

    // Verify the Claude event unsubscribe was called during cleanup
    expect(claudeUnsub).toHaveBeenCalled();
  });

  it('unmount calls sidecarCrashedUnsub cleanup function', async () => {
    const sidecarCrashedUnlisten = vi.fn();
    vi.mocked((bridge as any).subscribeToSidecarCrashed).mockResolvedValueOnce(sidecarCrashedUnlisten);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // sidecarCrashedUnsub should not have been called while component is alive
    expect(sidecarCrashedUnlisten).not.toHaveBeenCalled();

    // Unmount — onCleanup should call sidecarCrashedUnsub?.()
    unmount();

    // Verify the sidecar-crashed unsubscribe was called during cleanup
    expect(sidecarCrashedUnlisten).toHaveBeenCalled();
  });

  it('subscribeToSidecarCrashed is called exactly once after ready', async () => {
    await renderAndWaitForReady();
    expect((bridge as any).subscribeToSidecarCrashed).toHaveBeenCalledTimes(1);
  });

  it('does not leak Claude event listeners when unmounted before subscribeToClaudeEvents resolves', async () => {
    // Create a deferred promise for subscribeToClaudeEvents
    const unlistenClaude = vi.fn();
    const { promise: claudeSubPromise, resolve: resolveClaudeSub } = deferred<() => void>();
    vi.mocked(bridge.subscribeToClaudeEvents).mockReturnValue(claudeSubPromise);

    const { unmount } = render(() => <App />);

    // Wait for getInitialState to resolve and initApp to reach subscribeToClaudeEvents
    await flushMacrotasks();

    // Unmount while subscribeToClaudeEvents is still pending
    unmount();

    // Resolve the deferred subscribeToClaudeEvents — alive guard should fire
    resolveClaudeSub(unlistenClaude);

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // The alive guard (lines 260-263) calls unlistenClaude() and returns early,
    // never assigning claudeEventUnsub. So onCleanup's claudeEventUnsub?.() is a no-op.
    // The unlisten should be called exactly once — via the alive guard, not via onCleanup.
    expect(unlistenClaude).toHaveBeenCalledTimes(1);
  });
});

describe('App new component integration', () => {
  it('renders FileBrowser in the editor panel', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('file-browser')).toBeTruthy();
  });

  it('clicking Export in Toolbar opens ExportDialog', async () => {
    await renderAndWaitForReady();

    // ExportDialog should not be visible initially
    expect(screen.queryByTestId('export-dialog')).toBeNull();

    // Click Export in toolbar
    fireEvent.click(screen.getByText('Export'));

    // ExportDialog should now be visible
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });
  });

  it('ExportDialog Cancel closes the dialog', async () => {
    await renderAndWaitForReady();

    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    fireEvent.click(screen.getByText('Cancel'));
    await waitFor(() => {
      expect(screen.queryByTestId('export-dialog')).toBeNull();
    });
  });

  it('subscribes to file-changed events on mount', async () => {
    await renderAndWaitForReady();
    await waitFor(() => {
      expect(bridge.onFileChanged).toHaveBeenCalled();
    });
  });
});

describe('App navigation wiring', () => {
  const testState: GuiState = {
    meshes: [],
    values: [
      {
        cell_id: 'c1',
        name: 'width',
        value: '80',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'Bracket.width',
        kind: 'parameter',
        freshness: 'final',
      },
    ],
    constraints: [
      {
        node_id: 'n1',
        expression: 'width > 0',
        status: 'violated',
        label: null,
        parameter_ids: ['c1'],
      },
    ],
    files: [],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  it('viewport onSelect triggers getSourceLocation from bridge', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    render(() => <App />);

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    // Simulate viewport selection
    capturedDualViewportProps.onSelect('Bracket');

    await waitFor(() => {
      expect(bridge.getSourceLocation).toHaveBeenCalledWith('Bracket');
    });
  });

  it('App passes onGroupDoubleClick to PropertyEditor that calls bridge.focusEntity', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    render(() => <App />);

    // Wait for PropertyEditor to render with the values
    await waitFor(() => {
      expect(screen.getByText('Bracket')).toBeTruthy();
    });

    // Double-click the group header (this triggers onGroupDoubleClick)
    const bracketHeader = screen.getByText('Bracket');
    fireEvent.dblClick(bracketHeader);

    await waitFor(() => {
      expect(bridge.focusEntity).toHaveBeenCalledWith('Bracket');
    });
  });

  it('App passes onConstraintSelect to ConstraintPanel', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    render(() => <App />);

    // Wait for ConstraintPanel to render
    await waitFor(() => {
      expect(screen.getByTestId('constraint-row-n1')).toBeTruthy();
    });

    // Click a constraint row
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.click(row);

    // After clicking a constraint, the selectionStore should be updated.
    // The PropertyEditor should reflect highlighted params — the row c1 should get data-highlighted
    await waitFor(() => {
      const propRow = screen.getByTestId('prop-row-c1');
      expect(propRow.hasAttribute('data-highlighted')).toBe(true);
    });
  });

  it('selectionStore selectedEntity updates after viewport select', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    render(() => <App />);

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    // Simulate viewport selection
    capturedDualViewportProps.onSelect('Bracket');

    // Wait for getSourceLocation to resolve and selectEntity to be called
    await waitFor(() => {
      // The PropertyEditor should reflect the selection — Bracket group should be data-selected
      const container = screen.getByTestId('property-editor');
      const selectedGroups = container.querySelectorAll('[data-selected]');
      expect(selectedGroups.length).toBe(1);
    });
  });

  it('App subscribes to focus-entity events and dispatches to flyToEntity + selectionStore.selectEntity', async () => {
    let capturedFocusEntityCb: ((entityPath: string) => void) | undefined;
    vi.mocked(bridge.onFocusEntity).mockImplementation(async (cb) => {
      capturedFocusEntityCb = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);

    await renderAndWaitForReady();

    // The subscription must have been registered
    expect(vi.mocked(bridge.onFocusEntity)).toHaveBeenCalled();
    expect(capturedFocusEntityCb).toBeDefined();

    // Simulate focus-entity event from backend
    capturedFocusEntityCb!('Bracket');

    // mockFlyToEntity should be called (flyToEntityFn was wired via flyToEntityRef in Viewport mock)
    await waitFor(() => {
      expect(mockFlyToEntity).toHaveBeenCalledWith('Bracket');
    });

    // selectionStore.selectEntity should also have been called — PropertyEditor shows Bracket as data-selected
    await waitFor(() => {
      const container = screen.getByTestId('property-editor');
      const selectedGroups = container.querySelectorAll('[data-selected]');
      expect(selectedGroups.length).toBe(1);
      // Primary identity check: exact entity path routed through selectionStore to Viewport prop
      expect(capturedDualViewportProps.selectedEntity).toBe('Bracket');
    });
  });

  it('App subscribes to navigate-to-source events and updates Editor scrollToLocation signal with end_line/end_column from event', async () => {
    let capturedNavigateCb: ((data: { file: string; line: number; column: number; end_line: number; end_column: number }) => void) | undefined;
    vi.mocked(bridge.onNavigateToSource).mockImplementation(async (cb) => {
      capturedNavigateCb = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);

    await renderAndWaitForReady();

    expect(vi.mocked(bridge.onNavigateToSource)).toHaveBeenCalled();
    expect(capturedNavigateCb).toBeDefined();

    // Simulate navigate-to-source event from backend with distinct end positions
    capturedNavigateCb!({ file: '/project/bracket.ri', line: 12, column: 4, end_line: 18, end_column: 9 });

    // Editor's scrollToLocation signal should update with the full range from the event,
    // not end_line/end_column synthesized from line/column
    await waitFor(() => {
      const loc = capturedEditorScrollToLocation?.();
      expect(loc).toEqual({
        file_path: '/project/bracket.ri',
        line: 12,
        column: 4,
        end_line: 18,
        end_column: 9,
      });
    });
  });
});

describe('App initialization loading state', () => {
  it('shows app-loading while getInitialState is pending', async () => {
    // Create a deferred promise so getInitialState stays pending
    const { promise: getStatePromise, resolve: resolveGetState } = deferred<GuiState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(getStatePromise);

    render(() => <App />);

    // Should show loading indicator while pending
    expect(screen.getByTestId('app-loading')).toBeTruthy();
    // Should NOT show the main layout yet
    expect(screen.queryByTestId('app-layout')).toBeNull();

    // Resolve to transition to ready
    resolveGetState({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    expect(screen.queryByTestId('app-loading')).toBeNull();
  });

  it('shows app-error with retry button when getInitialState rejects', async () => {
    vi.mocked(bridge.getInitialState).mockRejectedValue(new Error('network error'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });
    // Should have a retry button
    expect(screen.getByText('Retry')).toBeTruthy();
    // Should NOT show loading or main layout
    expect(screen.queryByTestId('app-loading')).toBeNull();
    expect(screen.queryByTestId('app-layout')).toBeNull();
  });

  it('clicking retry button calls getInitialState again', async () => {
    // First call rejects
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Reset to succeed on retry
    vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });

    fireEvent.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // getInitialState called twice: initial + retry
    expect(bridge.getInitialState).toHaveBeenCalledTimes(2);
  });

  it('after successful getInitialState, app-layout is shown and loading/error are gone', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    expect(screen.queryByTestId('app-loading')).toBeNull();
    expect(screen.queryByTestId('app-error')).toBeNull();
  });
});

describe('App toast queue (TO-2)', () => {
  async function triggerExport() {
    // Wait for app to reach ready state before interacting
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    // Open export dialog via toolbar Export button
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => expect(screen.getByTestId('export-dialog')).toBeTruthy());
    // Click the Export button inside the dialog (not the toolbar one)
    const dialog = screen.getByTestId('export-dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Export' }));
  }

  it('successful export renders a toast in the queue', async () => {
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);
    render(() => <App />);
    await triggerExport();
    await waitFor(() => {
      expect(screen.getByTestId('toast')).toBeTruthy();
    });
  });

  it('two sequential toasts are both visible simultaneously', async () => {
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);
    render(() => <App />);

    await triggerExport();
    await waitFor(() => {
      expect(screen.getAllByTestId('toast').length).toBeGreaterThanOrEqual(1);
    });

    await triggerExport();
    await waitFor(() => {
      expect(screen.getAllByTestId('toast').length).toBe(2);
    });
  });

  it('dismissing one toast removes only that toast', async () => {
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);
    render(() => <App />);

    await triggerExport();
    await waitFor(() => expect(screen.getAllByTestId('toast').length).toBeGreaterThanOrEqual(1));

    await triggerExport();
    await waitFor(() => expect(screen.getAllByTestId('toast').length).toBe(2));

    // Dismiss first toast via Close button
    const closeButtons = screen.getAllByLabelText('Close');
    fireEvent.click(closeButtons[0]);
    await waitFor(() => expect(screen.getAllByTestId('toast').length).toBe(1));
  });
});

describe('App changedFiles multi-file tracking (R-1)', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));
  });

  it('two different file changes show both in reload prompt', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Simulate two different files changing
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => {
      expect(screen.getByText(/2 files changed/)).toBeTruthy();
    });
  });

  it('same file changed twice results in only one entry', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });

    await waitFor(() => {
      expect(screen.getByTestId('reload-prompt')).toBeTruthy();
    });
    // Should show single file, not "2 files changed"
    const reloadPrompt = screen.getByTestId('reload-prompt');
    expect(reloadPrompt.textContent).toMatch(/bracket\.ri/);
    expect(reloadPrompt.textContent).not.toMatch(/2 files changed/);
  });

  it('handleReload reloads all files in the changed set', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    // Click Reload
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
      expect(bridge.openFile).toHaveBeenCalledWith('/project/gear.ri');
    });
  });

  it('handleDismissReload clears all changed files', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    // Click Dismiss
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });
  });
});

describe('App dirty-file check before reload (R-4)', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));
  });

  it('when no dirty files overlap, handleReload proceeds immediately', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');

    // No dirty files — Reload should proceed immediately
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });
  });

  it('when dirty files overlap with changed files, shows confirmation warning instead of reloading', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark bracket.ri as dirty (unsaved changes)
    capturedEditorStore.markDirty('/project/bracket.ri');

    // Trigger file change for the dirty file
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    // Click Reload — should show confirmation warning, NOT call bridgeOpenFile
    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(screen.getByText(/Unsaved changes will be lost/)).toBeTruthy();
      expect(screen.getByText('Reload Anyway')).toBeTruthy();
    });

    // bridgeOpenFile should NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });

  it('confirming Reload Anyway triggers bridgeOpenFile for all changed files', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    // First click shows warning
    fireEvent.click(screen.getByText('Reload'));
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

    // Click Reload Anyway — should proceed
    fireEvent.click(screen.getByText('Reload Anyway'));

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
      expect(bridge.openFile).toHaveBeenCalledWith('/project/gear.ri');
    });
  });

  it('dismissing confirmation does not reload', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

    // First click shows warning
    fireEvent.click(screen.getByText('Reload'));
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

    // Click Dismiss
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // bridgeOpenFile should NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

describe('App handleReload partial failure', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('when one file succeeds and another fails, only the failed file remains in changedFiles', async () => {
    // bracket.ri succeeds, gear.ri fails
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // After allSettled, only gear.ri should remain (bracket.ri succeeded)
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        expect(prompt.textContent).toMatch(/gear\.ri/);
        expect(prompt.textContent).not.toMatch(/bracket\.ri/);
        expect(prompt.textContent).not.toMatch(/2 files changed/);
      });
    });
  });

  it('when one file fails, confirmReload is still reset to false', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Mark gear.ri as dirty so we enter confirmReload flow
      capturedEditorStore.markDirty('/project/gear.ri');

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      // First click shows confirmation warning
      fireEvent.click(screen.getByText('Reload'));
      await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());

      // Confirm reload
      fireEvent.click(screen.getByText('Reload Anyway'));

      // After partial failure, confirmReload should be reset
      // (so the remaining gear.ri prompt shows normal Reload, not Reload Anyway)
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        // gear.ri remains but confirmReload was reset — should NOT show 'Reload Anyway'
        expect(prompt.textContent).toMatch(/gear\.ri/);
        expect(prompt.textContent).not.toMatch(/Reload Anyway/);
      });
    });
  });

  it('when one file fails, an error toast is shown mentioning the failure count', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => {
      if (path === '/project/gear.ri') {
        throw new Error('disk read error');
      }
      return { path, content: `updated ${path}` };
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // An error toast should appear indicating reload failure
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.textContent?.match(/failed.*reload/i));
        expect(errorToast).toBeTruthy();
      });
    });
  });

  it('when all files succeed, changedFiles is cleared completely', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    fireEvent.click(screen.getByText('Reload'));

    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });
  });

  it('when all files fail, all files remain in changedFiles', async () => {
    vi.mocked(bridge.openFile).mockImplementation(async (_path: string) => {
      throw new Error('disk error');
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      fireEvent.click(screen.getByText('Reload'));

      // After all failures, both files should remain and an error toast should appear
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.textContent?.match(/failed.*reload/i));
        expect(errorToast).toBeTruthy();
      });

      // Both files should still be in changedFiles
      expect(screen.getByText(/2 files changed/)).toBeTruthy();
    });
  });
});

describe('App handleReload race condition', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
      { path: '/project/gear.ri', content: 'structure Gear {}' },
      { path: '/project/housing.ri', content: 'structure Housing {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('concurrent file-change event during reload is preserved after all succeed', async () => {
    // Use deferred promises to control when bridgeOpenFile resolves
    const bracketOpen = deferred<any>();
    const gearOpen = deferred<any>();

    vi.mocked(bridge.openFile).mockImplementation((path: string) => {
      if (path === '/project/bracket.ri') return bracketOpen.promise;
      if (path === '/project/gear.ri') return gearOpen.promise;
      return Promise.resolve({ path, content: `updated ${path}` });
    });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Two files change
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    capturedEditorStore.markDirty('/project/gear.ri');
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    capturedEditorStore.markClean('/project/bracket.ri');
    capturedEditorStore.markClean('/project/gear.ri');

    // Click Reload — starts in-flight reload for bracket.ri and gear.ri
    fireEvent.click(screen.getByText('Reload'));

    // Wait for bridgeOpenFile to be called
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledTimes(2);
    });

    // DURING the in-flight reload, a new file-change event arrives for housing.ri
    capturedEditorStore.markDirty('/project/housing.ri');
    fileChangedCallback!({ path: '/project/housing.ri', content: '' });

    // Now resolve both promises (both succeed)
    bracketOpen.resolve({ path: '/project/bracket.ri', content: 'updated bracket.ri' });
    gearOpen.resolve({ path: '/project/gear.ri', content: 'updated gear.ri' });

    // After settlement, housing.ri should still be in changedFiles
    // The reload prompt should show housing.ri, not disappear entirely
    await waitFor(() => {
      const prompt = screen.getByTestId('reload-prompt');
      expect(prompt.textContent).toMatch(/housing\.ri/);
    });
  });

  it('concurrent file-change event during reload preserved alongside partial failure', async () => {
    // bracket.ri succeeds, gear.ri fails, housing.ri arrives concurrently
    const bracketOpen = deferred<any>();
    const gearOpen = deferred<any>();

    vi.mocked(bridge.openFile).mockImplementation((path: string) => {
      if (path === '/project/bracket.ri') return bracketOpen.promise;
      if (path === '/project/gear.ri') return gearOpen.promise;
      return Promise.resolve({ path, content: `updated ${path}` });
    });

    await withSuppressedRejectionsAndErrorSpy(async () => {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Two files change
      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      capturedEditorStore.markDirty('/project/gear.ri');
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      capturedEditorStore.markClean('/project/bracket.ri');
      capturedEditorStore.markClean('/project/gear.ri');

      // Click Reload
      fireEvent.click(screen.getByText('Reload'));

      await waitFor(() => {
        expect(bridge.openFile).toHaveBeenCalledTimes(2);
      });

      // Concurrent file-change event during in-flight reload
      capturedEditorStore.markDirty('/project/housing.ri');
      fileChangedCallback!({ path: '/project/housing.ri', content: '' });

      // Resolve bracket (success), reject gear (failure)
      bracketOpen.resolve({ path: '/project/bracket.ri', content: 'updated bracket.ri' });
      gearOpen.reject(new Error('disk error'));

      // After settlement: gear.ri (failed) + housing.ri (concurrent) should both remain
      // bracket.ri (succeeded) should be removed
      await waitFor(() => {
        const prompt = screen.getByTestId('reload-prompt');
        expect(prompt.textContent).toMatch(/2 files changed/);
      });
    });
  });
});

describe('App handleSetParameter error handling', () => {
  it('shows error toast when bridge.setParameter rejects', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('backend unavailable'));

      vi.mocked(bridge.getInitialState).mockResolvedValue({
        meshes: [],
        values: [{
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
          kind: 'parameter',
          freshness: 'final',
        }],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      // Wait for PropertyEditor to show the value
      await waitFor(() => {
        expect(screen.getByText('width')).toBeTruthy();
      });

      // Find the input and press Enter to trigger onSetParameter
      const row = screen.getByTestId('prop-row-c1');
      const input = row.querySelector('input[type="text"]') as HTMLInputElement;
      expect(input).toBeTruthy();

      fireEvent.keyDown(input, { key: 'Enter' });

      // Wait for the error toast to appear
      await waitFor(() => {
        const toast = screen.getByTestId('toast');
        expect(toast).toBeTruthy();
        expect(toast.dataset.type).toBe('error');
        expect(toast.textContent).toContain('Parameter update failed');
        expect(toast.textContent).toContain('backend unavailable');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'setParameter failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App re-evaluate error toast', () => {
  it('shows error toast when re-evaluate (F5) fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.updateSource).mockRejectedValue(new Error('eval error'));
      vi.mocked(bridge.getInitialState).mockResolvedValue({
        meshes: [],
        values: [],
        constraints: [],
        files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      // Wait for ready state
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Press F5 to trigger re-evaluate (on a non-input element)
      fireEvent.keyDown(document, { key: 'F5' });

      // Wait for the error toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.dataset.type).toBe('error');
        expect(toastEl.textContent).toContain('Re-evaluation failed');
        expect(toastEl.textContent).toContain('eval error');
      });

      // console.error should NOT be called
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Re-evaluate failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App F5 re-evaluate multi-file', () => {
  it('F5 re-evaluate sends only the active file content when multiple files are open', async () => {
    // Arrange: two files — after init, mount.ri is activeFile (last opened)
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [],
      values: [],
      constraints: [],
      files: [
        { path: '/project/bracket.ri', content: 'structure Bracket {}' },
        { path: '/project/mount.ri', content: 'structure Mount {}' },
      ],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });
    vi.mocked(bridge.updateSource).mockResolvedValue(undefined as any);

    render(() => <App />);

    // Wait for ready state (both files opened, activeFile = '/project/mount.ri')
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Modify the non-active file's content via the captured editor store
    capturedEditorStore!.updateFileContent('/project/bracket.ri', 'MODIFIED CONTENT');
    // Also modify the active file's content — F5 should send this updated content, not a stale snapshot
    capturedEditorStore!.updateFileContent('/project/mount.ri', 'structure Mount { updated: true }');

    // Clear any prior updateSource calls (e.g. from initial file load) so the assertion is self-contained
    vi.mocked(bridge.updateSource).mockClear();

    // Act: press F5 to trigger handleReEvaluate
    fireEvent.keyDown(document, { key: 'F5' });

    // Assert: updateSource called exactly once with the ACTIVE file (mount.ri) and its CURRENT content
    await waitFor(() => {
      expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledTimes(1);
    });
    expect(vi.mocked(bridge.updateSource)).toHaveBeenCalledWith(
      '/project/mount.ri',
      'structure Mount { updated: true }',
    );
    // The non-active file (bracket.ri) must not have been sent
    expect(vi.mocked(bridge.updateSource)).not.toHaveBeenCalledWith(
      '/project/bracket.ri',
      expect.anything(),
    );
  });
});

describe('App event subscription error toast', () => {
  it('shows warning toast when subscribeToEvents fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      // Make onMeshUpdate throw synchronously — this causes subscribeToEvents
      // to reject because the array literal throws before Promise.allSettled runs
      vi.mocked(bridge.onMeshUpdate).mockImplementation(() => {
        throw new Error('subscription failed');
      });

      vi.mocked(bridge.getInitialState).mockResolvedValue({
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      // Wait for ready state (subscribeToEvents failure is non-fatal)
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Wait for the warning toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.textContent).toContain('Event subscription failed');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Failed to subscribe to events:',
        expect.any(Error),
      );
    });
  });
});

describe('App Claude subscription error toast', () => {
  it('shows warning toast when subscribeToClaudeEvents rejects', async () => {
    vi.mocked(bridge.subscribeToClaudeEvents).mockRejectedValueOnce(
      new Error('connection refused'),
    );

    render(() => <App />);

    // Wait for ready state (subscribeToClaudeEvents failure is non-fatal)
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Wait for the Claude-specific warning toast to appear
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const claudeToast = toasts.find((t) =>
        t.textContent?.includes('Claude assistant unavailable'),
      );
      expect(claudeToast).toBeTruthy();
      expect(claudeToast!.textContent).toContain(
        'chat features may not work',
      );
    });
  });
});

describe('App reload error toast', () => {
  it('shows error toast when reload fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      // Set up a file-changed callback we can trigger
      let fileChangedCb!: (data: any) => void;
      vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
        fileChangedCb = cb;
        return () => {};
      });

      // Make bridgeOpenFile reject
      vi.mocked(bridge.openFile).mockRejectedValue(new Error('file not found'));

      vi.mocked(bridge.getInitialState).mockResolvedValue({
        meshes: [],
        values: [],
        constraints: [],
        files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      // Wait for ready state and file-changed subscription
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
        expect(fileChangedCb).toBeDefined();
      });
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Trigger file changed event to show the reload prompt
      capturedEditorStore.markDirty('/project/bracket.ri');
      fileChangedCb({ path: '/project/bracket.ri', content: 'updated' });

      await waitFor(() => {
        expect(screen.getByTestId('reload-prompt')).toBeTruthy();
      });

      capturedEditorStore.markClean('/project/bracket.ri');

      // Click the Reload button
      fireEvent.click(screen.getByText('Reload'));

      // Wait for the error toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.dataset.type).toBe('error');
        expect(toastEl.textContent).toContain('failed to reload');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Reload failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App file picker integration (E-6)', () => {
  it('calls pickSavePath then exportGeometry with the chosen path', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/export.step');
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog (default format is 'step')
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('export.step', 'step');
    });

    await waitFor(() => {
      expect(bridge.exportGeometry).toHaveBeenCalledWith('step', '/user/chosen/export.step');
    });
  });

  it('does NOT call exportGeometry when user cancels file picker', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(null);
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });

    // Give time for any downstream calls
    await new Promise((r) => setTimeout(r, 50));

    // exportGeometry should NOT have been called
    expect(bridge.exportGeometry).not.toHaveBeenCalled();

    // Dialog should still be open (not closed after cancel)
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
  });
});

describe('App pickSavePath error boundary', () => {
  it('shows error toast and keeps dialog open when pickSavePath rejects', async () => {
    vi.mocked(bridge.pickSavePath).mockRejectedValue(new Error('Plugin not registered'));
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    await renderAndWaitForReady();

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:last-of-type') as HTMLButtonElement;
    fireEvent.click(exportBtn);

    // Wait for the rejection to be handled
    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });

    // Give time for any async handling
    await new Promise((r) => setTimeout(r, 50));

    // (1) Error toast should be shown with message about save dialog failure
    await waitFor(() => {
      expect(screen.getByText(/Could not open save dialog/)).toBeTruthy();
    });

    // (2) bridgeExportGeometry should NOT have been called
    expect(bridge.exportGeometry).not.toHaveBeenCalled();

    // (3) Dialog should still be open and NOT in exporting state (no spinner)
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
    expect(screen.queryByTestId('export-spinner')).toBeNull();
  });
});

describe('App initApp concurrent execution guard', () => {
  it('rapid double-click on Retry does not start two concurrent initApp flights', async () => {
    // First getInitialState rejects → error state
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Set up deferred promise for retry (keeps initApp in-flight)
    const { promise: retryPromise, resolve: resolveRetry } = deferred<GuiState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(retryPromise);

    // Click Retry — first retry
    fireEvent.click(screen.getByText('Retry'));

    // Immediately after click, the Retry button should be either disabled or
    // removed from DOM, preventing a second click from firing.
    const retryBtn = screen.queryByText('Retry');
    expect(retryBtn === null || (retryBtn as HTMLButtonElement).disabled).toBe(true);

    // getInitialState should be called exactly twice: initial mount + first retry
    // NOT three times (which would indicate double-click succeeded)
    expect(bridge.getInitialState).toHaveBeenCalledTimes(2);

    // Clean up: resolve the deferred promise
    resolveRetry({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
  });

  it('retry cleans up prior subscriptions before re-subscribing', async () => {
    // Track unsub calls with call order tracking
    const callLog: string[] = [];
    const priorUnsub = vi.fn(() => callLog.push('prior-unsub'));
    const priorFileUnsub = vi.fn(() => callLog.push('prior-file-unsub'));
    const newMeshUnsub = vi.fn(() => callLog.push('new-mesh-unsub'));
    const newValueUnsub = vi.fn(() => callLog.push('new-value-unsub'));
    const newConstraintUnsub = vi.fn(() => callLog.push('new-constraint-unsub'));
    const newEvalUnsub = vi.fn(() => callLog.push('new-eval-unsub'));
    const newMeshRmUnsub = vi.fn(() => callLog.push('new-mesh-rm-unsub'));
    const newValueRmUnsub = vi.fn(() => callLog.push('new-value-rm-unsub'));
    const newConstraintRmUnsub = vi.fn(() => callLog.push('new-constraint-rm-unsub'));
    const newFileUnsub = vi.fn(() => callLog.push('new-file-unsub'));

    // First initApp (mount): getInitialState succeeds, subs established
    // onMeshUpdate returns the "prior" unsub — subscribeToEvents bundles it
    vi.mocked(bridge.onMeshUpdate).mockResolvedValueOnce(priorUnsub);
    vi.mocked(bridge.onFileChanged).mockResolvedValueOnce(priorFileUnsub);

    vi.mocked(bridge.getInitialState).mockResolvedValueOnce({
      meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Verify prior subscriptions are active (unsubs not yet called)
    expect(priorUnsub).not.toHaveBeenCalled();
    expect(priorFileUnsub).not.toHaveBeenCalled();

    // Set up new mocks for a second initApp call (if it were to happen)
    vi.mocked(bridge.onMeshUpdate).mockResolvedValue(newMeshUnsub);
    vi.mocked(bridge.onValueUpdate).mockResolvedValue(newValueUnsub);
    vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(newConstraintUnsub);
    vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(newEvalUnsub);
    vi.mocked(bridge.onMeshRemoved).mockResolvedValue(newMeshRmUnsub);
    vi.mocked(bridge.onValueRemoved).mockResolvedValue(newValueRmUnsub);
    vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(newConstraintRmUnsub);
    vi.mocked(bridge.onFileChanged).mockResolvedValue(newFileUnsub);

    // Unmount — cleanup should call both the composite unsub (which calls
    // priorUnsub) and fileChangedUnsub (priorFileUnsub)
    unmount();

    // All prior subscription cleanup functions should have been called
    expect(priorUnsub).toHaveBeenCalled();
    expect(priorFileUnsub).toHaveBeenCalled();

    // Verify cleanup happened — the prior unsubs should be in the call log
    expect(callLog).toContain('prior-unsub');
    expect(callLog).toContain('prior-file-unsub');
  });

  it('Retry button is disabled while initApp is in-flight (loading phase)', async () => {
    // First getInitialState rejects → error state
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Retry button should be present and clickable in error state
    const retryBtn = screen.getByText('Retry') as HTMLButtonElement;
    expect(retryBtn.disabled).toBe(false);

    // Set up deferred getInitialState so initApp stays in loading phase
    const { promise: retryPromise, resolve: resolveRetry } = deferred<GuiState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(retryPromise);

    // Click Retry — should transition to loading phase
    fireEvent.click(retryBtn);

    // The Retry button should no longer be in the DOM (loading phase hides
    // the error state) or should be disabled to prevent re-clicks
    expect(screen.queryByText('Retry')).toBeNull();

    // Also verify we're in loading state
    expect(screen.getByTestId('app-loading')).toBeTruthy();

    // Clean up: resolve the deferred promise
    resolveRetry({ meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [], tensegrity_wires: [] });
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
  });
});

describe('App layout persistence', () => {
  it('panel widths initialize from localStorage when valid data exists', async () => {
    const layout = { editorWidth: 400, sideWidth: 350, propertyHeight: 250 };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));

    await renderAndWaitForReady();

    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();
    // Grid columns should reflect stored values
    expect(main.style.gridTemplateColumns).toContain('400px');
    expect(main.style.gridTemplateColumns).toContain('350px');
  });

  it('missing localStorage falls back to defaults (300/300/200)', async () => {
    await renderAndWaitForReady();

    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();
    expect(main.style.gridTemplateColumns).toContain('300px');
    // Side panel should also default to 300px
    const cols = main.style.gridTemplateColumns;
    // Should have 300px ... 300px (editor and side panel widths)
    const matches = cols.match(/(\d+)px/g);
    expect(matches).toContain('300px');
  });

  it('resizing left splitter writes updated layout to localStorage', async () => {
    await renderAndWaitForReady();

    const splitter = screen.getByTestId('splitter-left');

    // Drag right by 50px
    fireEvent.mouseDown(splitter, { clientX: 300, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 350, clientY: 200 });
    fireEvent.mouseUp(document);

    // Wait for debounced save (300ms debounce + margin)
    await new Promise((r) => setTimeout(r, 400));

    const stored = localStorage.getItem(STORAGE_KEY);
    expect(stored).not.toBeNull();
    const parsed = JSON.parse(stored!);
    expect(parsed.editorWidth).toBe(350);
  });
});

describe('App viewport prop wiring', () => {
  it('capturedDualViewportProps.onHover is a function that updates selectionStore', async () => {
    await renderAndWaitForReady();

    expect(capturedDualViewportProps.onHover).toBeDefined();
    expect(typeof capturedDualViewportProps.onHover).toBe('function');

    // Calling onHover should update selectionStore.hoveredEntity
    capturedDualViewportProps.onHover('bracket/hole');

    // The hoveredEntity should now be passed back to Viewport (verified via capturedViewportProps)
    await waitFor(() => {
      expect(capturedDualViewportProps.hoveredEntity).toBe('bracket/hole');
    });
  });

  it('capturedDualViewportProps.evalStatus is defined and reflects engine store state', async () => {
    await renderAndWaitForReady();

    expect(capturedDualViewportProps.evalStatus).toBeDefined();
    // Default engine store eval status is idle
    expect(capturedDualViewportProps.evalStatus.phase).toBe('idle');
  });
});

describe('App splitter max bounds', () => {
  it('dragging left splitter far right clamps editor width so viewport and side panel remain visible', async () => {
    await renderAndWaitForReady();
    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();

    // Mock container width (jsdom has 0 by default)
    Object.defineProperty(main, 'clientWidth', { value: 1200, configurable: true });

    const splitter = screen.getByTestId('splitter-left');

    // Drag right by a huge amount — should be clamped
    fireEvent.mouseDown(splitter, { clientX: 300, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 1500, clientY: 200 });
    fireEvent.mouseUp(document);

    // Editor width should be clamped: containerWidth(1200) - sideWidth(300) - MIN_PANEL_WIDTH(150) - 8(splitters)
    // = 742. So editorWidth should be <= 742
    const cols = main.style.gridTemplateColumns;
    const editorPx = parseInt(cols.split('px')[0], 10);
    expect(editorPx).toBeLessThanOrEqual(1200 - 300 - 150 - 8);
    expect(editorPx).toBeGreaterThan(0);
  });

  it('dragging right splitter far left clamps side panel width similarly', async () => {
    await renderAndWaitForReady();
    const main = screen.getByTestId('app-layout').querySelector('[class*="main"]') as HTMLElement;
    expect(main).toBeTruthy();

    Object.defineProperty(main, 'clientWidth', { value: 1200, configurable: true });

    const splitter = screen.getByTestId('splitter-right');

    // Drag left by a huge amount (negative delta for right splitter increases sideWidth)
    fireEvent.mouseDown(splitter, { clientX: 900, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 100, clientY: 200 });
    fireEvent.mouseUp(document);

    // Side panel width should be clamped: containerWidth(1200) - editorWidth(300) - MIN_PANEL_WIDTH(150) - 8
    // = 742
    const cols = main.style.gridTemplateColumns;
    const parts = cols.match(/(\d+)px/g)!;
    const sidePx = parseInt(parts[parts.length - 1], 10);
    expect(sidePx).toBeLessThanOrEqual(1200 - 300 - 150 - 8);
    expect(sidePx).toBeGreaterThan(0);
  });

  it('dragging side-panel splitter downward clamps property height', async () => {
    await renderAndWaitForReady();
    const sidePanel = screen.getByTestId('side-panel');
    const splitter = sidePanel.querySelector('[data-testid="splitter-side"]') as HTMLElement;
    expect(splitter).toBeTruthy();

    // Mock side panel height
    Object.defineProperty(sidePanel, 'clientHeight', { value: 600, configurable: true });

    // Drag down by a huge amount
    fireEvent.mouseDown(splitter, { clientX: 500, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 500, clientY: 1000 });
    fireEvent.mouseUp(document);

    // Property height should be clamped so the other panels (design tree,
    // constraint, chat) and their splitters still fit.
    const rows = sidePanel.style.gridTemplateRows;
    // Layout with chat open:
    //   <designTreeHeight>px 4px <propertyHeight>px 4px <constraintHeight>px 4px minmax(160px, 1fr)
    expect(rows).toMatch(/^\d+px 4px \d+px 4px \d+px 4px minmax\(160px, 1fr\)$/);
    // Standalone `Npx ` tokens are [designTree, splitter, property, splitter, constraint, splitter].
    const standalonePx = rows.match(/\b(\d+)px(?=\s)/g)!.map((s) => parseInt(s, 10));
    const [designTreeHeight, , propertyHeight, , constraintHeight] = standalonePx;
    // Property height must leave room for design tree, constraint, chat floor, and 3 splitters
    expect(propertyHeight).toBeLessThanOrEqual(600 - designTreeHeight - constraintHeight - 160 - 3 * 4);
    expect(propertyHeight).toBeGreaterThan(0);
  });

  it('clamps oversized persisted heights when ResizeObserver fires after mount', async () => {
    // Repro of the live bug: persisted heights from `localStorage` sum to
    // more than the side-panel container's actual height, so the chat panel
    // is pushed off-screen at first paint. The ResizeObserver attached to
    // `sidePanelRef` should re-clamp once the container's size is known.
    const oversize = {
      editorWidth: 900,
      sideWidth: 680,
      designTreeHeight: 584,
      propertyHeight: 508,
      constraintHeight: 150,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(oversize));

    // Capture the ResizeObserver constructed during App mount so the test
    // can fire the callback at a known time after `clientHeight` is mocked.
    let roCallback: ResizeObserverCallback | undefined;
    const OrigRO = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
      constructor(cb: ResizeObserverCallback) { roCallback = cb; }
    } as any;

    try {
      await renderAndWaitForReady();
      const sidePanel = screen.getByTestId('side-panel');
      // Mock the container height seen on the bug-repro window.
      Object.defineProperty(sidePanel, 'clientHeight', { value: 817, configurable: true });

      // Trigger the observer callback with a synthesised entry — the helper
      // re-reads `clientHeight` from the ref so the entry payload is unused.
      expect(roCallback).toBeDefined();
      roCallback!([{ contentRect: { width: 680, height: 817 } }] as any, {} as any);

      const rows = sidePanel.style.gridTemplateRows;
      expect(rows).toMatch(/^\d+px 4px \d+px 4px \d+px 4px minmax\(160px, 1fr\)$/);
      const standalonePx = rows.match(/\b(\d+)px(?=\s)/g)!.map((s) => parseInt(s, 10));
      const [designTreeH, , propertyH, , constraintH] = standalonePx;

      // Three sub-panels + chat floor (160) + 3 splitters (12) ≤ container.
      expect(designTreeH + propertyH + constraintH + 160 + 12).toBeLessThanOrEqual(817);
    } finally {
      globalThis.ResizeObserver = OrigRO;
    }
  });
});

describe('App fit-to-view wiring', () => {
  it('capturedDualViewportProps.fitToViewRef is defined', async () => {
    await renderAndWaitForReady();
    expect(capturedDualViewportProps.fitToViewRef).toBeDefined();
    expect(typeof capturedDualViewportProps.fitToViewRef).toBe('function');
  });

  it('Toolbar Fit to View click triggers viewport fitToView via fitToViewRef', async () => {
    await renderAndWaitForReady();

    // The Viewport mock invokes fitToViewRef with mockViewportFitToView.
    // App stores it. Clicking Fit to View in Toolbar should call it.
    mockViewportFitToView.mockClear();

    const fitBtn = screen.getByText('Fit to View');
    fireEvent.click(fitBtn);

    expect(mockViewportFitToView).toHaveBeenCalled();
  });
});

describe('App Ctrl+O open file', () => {
  it('App wires handleOpen into Editor.onOpen (invoking it calls bridge.pickOpenPath)', async () => {
    const pickSpy = vi.spyOn(bridge, 'pickOpenPath').mockResolvedValue(null);
    await renderAndWaitForReady();

    // capturedEditorOnOpen should be wired to App's handleOpen
    await vi.waitFor(() => {
      expect(capturedEditorOnOpen).toBeDefined();
    });

    // Invoke the captured callback — should trigger bridge.pickOpenPath
    capturedEditorOnOpen!();

    await vi.waitFor(() => {
      expect(pickSpy).toHaveBeenCalledTimes(1);
    });
  });

  it('dispatching Ctrl+O triggers pickOpenPath then openFile', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [], values: [], constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });

    // Mock pickOpenPath to return a path
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Press Ctrl+O
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    // Should call pickOpenPath, then openFile with the returned path
    await waitFor(() => {
      expect(bridge.pickOpenPath).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/other.ri');
    });
  });
});

describe('App handleOpen dirty-check confirmation', () => {
  function setupHappyPathMocks() {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });
    vi.mocked(bridge.openFileEngine).mockResolvedValue({
      meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });
  }

  it('Ctrl+O with dirty buffer and confirm cancelled: pickOpenPath not called', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushMacrotasks();
      expect(confirmSpy).toHaveBeenCalledTimes(1);
      expect(bridge.pickOpenPath).not.toHaveBeenCalled();
      expect(bridge.openFile).not.toHaveBeenCalled();
      expect(bridge.openFileEngine).not.toHaveBeenCalled();
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it('Ctrl+O with dirty buffer and confirm accepted: bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await waitFor(() => {
        expect(bridge.pickOpenPath).toHaveBeenCalled();
      });
      await waitFor(() => {
        expect(bridge.openFile).toHaveBeenCalledWith('/project/other.ri');
      });
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it('Ctrl+O with dirty buffer, confirm accepted, and pickOpenPath returns null: openFile/openFileEngine not called', async () => {
    setupHappyPathMocks();
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(null);
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushMacrotasks();
      expect(confirmSpy).toHaveBeenCalledTimes(1);
      expect(bridge.pickOpenPath).toHaveBeenCalledTimes(1);
      expect(bridge.openFile).not.toHaveBeenCalled();
      expect(bridge.openFileEngine).not.toHaveBeenCalled();
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it('Ctrl+O with clean buffer: window.confirm not called, bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    // No markDirty — buffer is clean

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await waitFor(() => {
        expect(bridge.pickOpenPath).toHaveBeenCalled();
      });
      expect(confirmSpy).not.toHaveBeenCalled();
    } finally {
      confirmSpy.mockRestore();
    }
  });
});

describe('App File→New (Ctrl+N) save-as-you-go flow', () => {
  const newPath = '/user/chosen/new.ri';
  const newContent = NEW_FILE_TEMPLATE;

  function setupHappyPathMocks() {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFileEngine).mockResolvedValue({
      meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });
  }

  it('Ctrl+N happy path: calls pickSavePath, saveFile, openFile, openFileEngine in order', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(newPath, newContent);
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith(newPath);
    });
    await waitFor(() => {
      expect(bridge.openFileEngine).toHaveBeenCalledWith(newPath);
    });
  });

  it('File→New menu click: calls the same bridge sequence', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();

    fireEvent.click(screen.getByText('File'));
    const newItem = screen.getAllByRole('menuitem').find((el) => el.textContent?.includes('New'));
    expect(newItem).toBeTruthy();
    fireEvent.click(newItem!);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
    });
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(newPath, newContent);
    });
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith(newPath);
    });
  });

  it('cancel: pickSavePath returns null → saveFile and openFile not called', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(null);
    await renderAndWaitForReady();

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });
    await flushMacrotasks();

    expect(bridge.saveFile).not.toHaveBeenCalled();
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(bridge.openFileEngine).not.toHaveBeenCalled();
  });

  it('error: pickSavePath rejects → shows "New file failed" toast, saveFile not called', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockRejectedValue(new Error('Plugin not registered'));
      await renderAndWaitForReady();

      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

      await waitFor(() => {
        expect(screen.getByText(/New file failed/)).toBeTruthy();
      });
      expect(bridge.saveFile).not.toHaveBeenCalled();
    });
  });

  it('error: saveFile rejects → shows "New file failed" toast with error message, openFile not called', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
      vi.mocked(bridge.saveFile).mockRejectedValue(new Error('disk full'));
      await renderAndWaitForReady();

      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

      await waitFor(() => {
        expect(screen.getByText(/New file failed/)).toBeTruthy();
      });
      await waitFor(() => {
        expect(screen.getByText(/disk full/)).toBeTruthy();
      });
      expect(bridge.openFile).not.toHaveBeenCalled();
    });
  });

  it('editor store integration: after successful new-file flow, openFiles contains new path', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fireEvent.keyDown(document, { key: 'n', ctrlKey: true });

    await waitFor(() => {
      const paths = capturedEditorStore.state.openFiles.map((f: any) => f.path);
      expect(paths).toContain(newPath);
    });
  });
});

describe('App handleNew dirty-check confirmation', () => {
  const newPath = '/user/chosen/new.ri';
  const newContent = NEW_FILE_TEMPLATE;

  function setupHappyPathMocks() {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(newPath);
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: newPath, content: newContent });
    vi.mocked(bridge.openFileEngine).mockResolvedValue({
      meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    });
  }

  it('Ctrl+N with dirty buffer and confirm cancelled: pickSavePath not called', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    try {
      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
      await flushMacrotasks();
      expect(confirmSpy).toHaveBeenCalledTimes(1);
      expect(bridge.pickSavePath).not.toHaveBeenCalled();
      expect(bridge.saveFile).not.toHaveBeenCalled();
      expect(bridge.openFile).not.toHaveBeenCalled();
      expect(bridge.openFileEngine).not.toHaveBeenCalled();
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it('Ctrl+N with dirty buffer and confirm accepted: bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.markDirty('/project/bracket.ri');

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    try {
      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
      await waitFor(() => {
        expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
      });
      await waitFor(() => {
        expect(bridge.saveFile).toHaveBeenCalledWith(newPath, newContent);
      });
      await waitFor(() => {
        expect(bridge.openFile).toHaveBeenCalledWith(newPath);
      });
      await waitFor(() => {
        expect(bridge.openFileEngine).toHaveBeenCalledWith(newPath);
      });
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it('Ctrl+N with clean buffer: window.confirm not called, bridge sequence fires', async () => {
    setupHappyPathMocks();
    await renderAndWaitForReady();
    // No markDirty — buffer is clean

    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    try {
      fireEvent.keyDown(document, { key: 'n', ctrlKey: true });
      await waitFor(() => {
        expect(bridge.pickSavePath).toHaveBeenCalledWith('untitled.ri', 'ri');
      });
      expect(confirmSpy).not.toHaveBeenCalled();
    } finally {
      confirmSpy.mockRestore();
    }
  });
});

describe('App end-to-end toast integration', () => {
  it('App renders, loads state (ready), then setParameter failure shows toast with correct message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('backend unavailable'));
      vi.mocked(bridge.getInitialState).mockResolvedValue({
        meshes: [],
        values: [{
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
          kind: 'parameter',
          freshness: 'final',
        }],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      // Wait for ready state
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Verify no toast is visible initially
      expect(screen.queryByTestId('toast')).toBeNull();

      // Trigger setParameter failure
      const row = screen.getByTestId('prop-row-c1');
      const input = row.querySelector('input[type="text"]') as HTMLInputElement;
      expect(input).toBeTruthy();
      fireEvent.keyDown(input, { key: 'Enter' });

      // Wait for error toast to appear with correct message
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.dataset.type).toBe('error');
        expect(toastEl.textContent).toContain('Parameter update failed');
        expect(toastEl.textContent).toContain('backend unavailable');
      });
    });
  });
});

describe('App keyboard help overlay', () => {
  it('pressing ? key shows keyboard help overlay', async () => {
    await renderAndWaitForReady();

    // Help overlay should not be visible initially
    expect(screen.queryByTestId('keyboard-help')).toBeNull();

    // Press ? key
    fireEvent.keyDown(document, { key: '?' });

    await waitFor(() => {
      expect(screen.getByTestId('keyboard-help')).toBeTruthy();
    });
  });

  it('pressing ? again hides keyboard help (toggle behavior)', async () => {
    await renderAndWaitForReady();

    // Show help
    fireEvent.keyDown(document, { key: '?' });
    await waitFor(() => {
      expect(screen.getByTestId('keyboard-help')).toBeTruthy();
    });

    // Press ? again to hide
    fireEvent.keyDown(document, { key: '?' });
    await waitFor(() => {
      expect(screen.queryByTestId('keyboard-help')).toBeNull();
    });
  });

  it('pressing Escape while help is shown hides it', async () => {
    await renderAndWaitForReady();

    // Show help
    fireEvent.keyDown(document, { key: '?' });
    await waitFor(() => {
      expect(screen.getByTestId('keyboard-help')).toBeTruthy();
    });

    // Press Escape to close
    fireEvent.keyDown(document, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByTestId('keyboard-help')).toBeNull();
    });
  });
});

describe('App Claude error handling', () => {
  let consoleSpy: ReturnType<typeof vi.spyOn> | undefined;

  afterEach(() => {
    consoleSpy?.mockRestore();
    consoleSpy = undefined;
  });

  it('logs error to console when subscribeToClaudeEvents fails', async () => {
    const subscribeError = new Error('subscribe failed');
    vi.mocked(bridge.subscribeToClaudeEvents).mockRejectedValue(subscribeError);
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    await waitFor(() => {
      expect(consoleSpy).toHaveBeenCalledWith(
        '[claude] subscribeToClaudeEvents failed:',
        subscribeError,
      );
    });

    // Verify the toast DOM element appears with the correct error message
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const claudeToast = toasts.find((t) =>
        t.textContent?.includes('Claude assistant unavailable'),
      );
      expect(claudeToast).toBeTruthy();
      expect(claudeToast!.dataset.type).toBe('error');
      expect(claudeToast!.textContent).toContain('chat features may not work');
    });
  });

  it('shows toast when claudeAbort fails', async () => {
    const abortError = new Error('abort failed');
    vi.mocked(bridge.claudeAbort).mockRejectedValue(abortError);
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Capture the handler passed to subscribeToClaudeEvents
    let claudeHandler: ((msg: any) => void) | undefined;
    vi.mocked(bridge.subscribeToClaudeEvents).mockImplementation(async (handler) => {
      claudeHandler = handler;
      return () => {};
    });

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Guard: ensure subscribeToClaudeEvents was called and captured the handler
    await waitFor(() => expect(claudeHandler).toBeDefined());

    // Fire a text_delta event to put claudeStore into 'responding' state
    claudeHandler!({ type: 'text_delta', id: 'msg-1', content: 'Hello' });

    // Wait for abort button to appear
    await waitFor(() => {
      expect(screen.getByTestId('abort-button')).toBeTruthy();
    });

    // Click the abort button
    fireEvent.click(screen.getByTestId('abort-button'));

    // Wait for the toast to appear (claudeAbort rejection triggers async catch)
    await waitFor(() => {
      expect(consoleSpy).toHaveBeenCalledWith(
        '[claude] abort failed:',
        abortError,
      );
    });

    // Verify the toast DOM element matches the error pattern used by other error-path tests
    await waitFor(() => {
      const toastEl = screen.getByTestId('toast');
      expect(toastEl).toBeTruthy();
      expect(toastEl.dataset.type).toBe('error');
      expect(toastEl.textContent).toContain('Abort failed: abort failed');
    });
  });
});

describe('App onSend context forwarding', () => {
  it('forwards currentFile and attachedContexts to claudeSendMessage', async () => {
    // Set up initial state with a file so activeFile is set in ChatPanel
    const testState: GuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [{ path: 'bracket.ri', content: 'structure Bracket {}' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [],
    };
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Open context picker and attach 'file' context
    const pickerBtn = screen.getByTestId('context-picker-btn');
    fireEvent.click(pickerBtn);

    await waitFor(() => {
      expect(screen.getByTestId('context-picker-dropdown')).toBeTruthy();
    });

    // Click "Current file" option (4th button in the dropdown)
    const dropdown = screen.getByTestId('context-picker-dropdown');
    const options = dropdown.querySelectorAll('button');
    const fileOption = Array.from(options).find((btn) => btn.textContent === 'Current file');
    expect(fileOption).toBeTruthy();
    fireEvent.click(fileOption!);

    // Type a message in the chat input
    const chatInput = screen.getByTestId('chat-input');
    fireEvent.input(chatInput, { target: { value: 'help with this file' } });

    // Click send button
    const sendBtn = screen.getByTestId('send-button');
    fireEvent.click(sendBtn);

    // Verify claudeSendMessage was called with currentFile and attachedContexts
    await waitFor(() => {
      expect(bridge.claudeSendMessage).toHaveBeenCalledTimes(1);
    });

    const callArgs = vi.mocked(bridge.claudeSendMessage).mock.calls[0];
    const contextArg = callArgs[1];
    expect(contextArg).toBeDefined();
    expect(contextArg!.currentFile).toBe('bracket.ri');
    expect(contextArg!.attachedContexts).toContain('file');
  });
});

describe('App claudeSendMessage error-path integration', () => {
  it('claudeSendMessage failure renders system-message with ipc_error type and original error', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.claudeSendMessage).mockRejectedValueOnce(new Error('IPC channel broken'));
      vi.mocked(bridge.getInitialState).mockResolvedValueOnce({
        meshes: [],
        values: [],
        constraints: [],
        files: [{ path: 'bracket.ri', content: 'structure Bracket {}' }],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      });

      render(() => <App />);

      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Type a message in the chat input
      const chatInput = screen.getByTestId('chat-input');
      fireEvent.input(chatInput, { target: { value: 'help me' } });

      // Click send button to trigger onSend -> claudeSendMessage
      const sendBtn = screen.getByTestId('send-button');
      fireEvent.click(sendBtn);

      // Wait for the system-message element to appear (not a toast — this path uses addSystemMessage)
      await waitFor(() => {
        const sysMsg = screen.getByTestId('system-message');
        expect(sysMsg).toBeTruthy();
        expect(sysMsg.getAttribute('data-error-type')).toBe('ipc_error');
        expect(sysMsg.textContent).toContain('Failed to send message');
        expect(sysMsg.textContent).toContain('IPC channel broken');
      });

      // console.error should NOT be called (error is routed through addSystemMessage)
      expect(errorSpy).not.toHaveBeenCalledWith(
        '[claude] sendMessage failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App error-path integration: errorMessage propagation', () => {
  async function triggerExportDialog() {
    // Wait for app to reach ready state
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    // Open export dialog via toolbar Export button
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => expect(screen.getByTestId('export-dialog')).toBeTruthy());
    // Click the Export button inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Export' }));
  }

  it('export failure toast contains the original error message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.exportGeometry).mockRejectedValueOnce(new Error('geometry kernel crashed'));
      render(() => <App />);
      await triggerExportDialog();

      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.dataset.type === 'error');
        expect(errorToast).toBeTruthy();
        expect(errorToast!.textContent).toContain('Export failed');
        expect(errorToast!.textContent).toContain('geometry kernel crashed');
      });
    });
  });

  it('pickSavePath failure toast contains the original error message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockRejectedValueOnce(new Error('dialog permission denied'));
      render(() => <App />);
      await triggerExportDialog();

      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.dataset.type === 'error');
        expect(errorToast).toBeTruthy();
        expect(errorToast!.textContent).toContain('Could not open save dialog');
        expect(errorToast!.textContent).toContain('dialog permission denied');
      });
    });
  });
});

describe('App serialization-error subscription', () => {
  let serializationErrorCallback: ((data: { item_type: string; item_id: string; error: string }) => void) | undefined;

  beforeEach(() => {
    serializationErrorCallback = undefined;
    vi.mocked(bridge.onSerializationError).mockImplementation(async (cb: any) => {
      serializationErrorCallback = cb;
      return () => {};
    });
  });

  it('subscribes to serialization-error events and shows toast after debounce window', async () => {
    render(() => <App />);
    await waitFor(() => expect(serializationErrorCallback).toBeDefined());

    // Switch to fake timers so we can advance the 500ms debounce window instantly
    vi.useFakeTimers();
    try {
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32 value' });

      // Toast must NOT appear before the window elapses
      expect(screen.queryAllByTestId('toast').find((t) => t.dataset.type === 'error')).toBeUndefined();

      // Advance past the debounce window
      vi.advanceTimersByTime(500);
      // Let SolidJS flush any pending reactivity
      await Promise.resolve();

      const toasts = screen.getAllByTestId('toast');
      const errorToast = toasts.find((t) => t.dataset.type === 'error');
      expect(errorToast).toBeTruthy();
      expect(errorToast!.textContent).toContain("Failed to serialize mesh 'Bracket.body': non-finite f32 value");
    } finally {
      vi.useRealTimers();
    }
  });

  it('rapid-fire serialization errors produce a single summary toast', async () => {
    render(() => <App />);
    await waitFor(() => expect(serializationErrorCallback).toBeDefined());

    vi.useFakeTimers();
    try {
      // Fire multiple distinct errors within the debounce window
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'A', error: 'err1' });
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'B', error: 'err2' });
      serializationErrorCallback!({ item_type: 'value', item_id: 'C', error: 'err3' });

      // No toast yet
      expect(screen.queryAllByTestId('toast').find((t) => t.dataset.type === 'error')).toBeUndefined();

      vi.advanceTimersByTime(500);
      await Promise.resolve();

      const errorToasts = screen.getAllByTestId('toast').filter((t) => t.dataset.type === 'error');
      // Exactly one summary toast, not three individual toasts
      expect(errorToasts).toHaveLength(1);
      expect(errorToasts[0].textContent).toContain('3 items failed to serialize');
    } finally {
      vi.useRealTimers();
    }
  });

  it('cleans up serialization-error subscription on unmount', async () => {
    const serializationErrorUnsub = vi.fn();
    vi.mocked(bridge.onSerializationError).mockResolvedValueOnce(serializationErrorUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    expect(serializationErrorUnsub).not.toHaveBeenCalled();

    unmount();

    expect(serializationErrorUnsub).toHaveBeenCalled();
  });

  it('shows fallback toast when onSerializationError rejects', async () => {
    vi.mocked(bridge.onSerializationError).mockRejectedValueOnce(new Error('listen failed'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const errorToast = toasts.find((t) =>
        t.textContent?.includes('Serialization error monitoring unavailable'),
      );
      expect(errorToast).toBeTruthy();
    });
  });

  it('does not leak serialization-error listener when unmounted before onSerializationError resolves', async () => {
    const unlistenSerialization = vi.fn();
    const { promise, resolve } = deferred<() => void>();
    vi.mocked(bridge.onSerializationError).mockReturnValue(promise);

    const { unmount } = render(() => <App />);

    // Wait for initApp to reach the onSerializationError await
    await flushMacrotasks();

    // Unmount while onSerializationError is still pending
    unmount();

    // Resolve the deferred promise — alive guard should fire
    resolve(unlistenSerialization);

    // Flush so the alive guard's unlistenSerialization() call executes
    await flushMacrotasks();

    // The alive guard (App.tsx:267-269) calls it once; onCleanup's serializationErrorUnsub?.() is a no-op.
    expect(unlistenSerialization).toHaveBeenCalledTimes(1);
  });
});

describe('App handleSave dirty-indicator and error handling', () => {
  it('clears dirty indicator after successful save via Ctrl+S', async () => {
    const path = '/project/test.ri';
    const content = 'module Test {}';

    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);

    // Wait for App to be ready and capturedEditorStore to be set
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Open a file in the store, mark it dirty, and set as active
    capturedEditorStore.openFile({ path, content });
    capturedEditorStore.markDirty(path);
    capturedEditorStore.setActiveFile(path);

    // Confirm the file is dirty before save
    expect(capturedEditorStore.state.dirtyFiles).toContain(path);

    // Trigger handleSave via global Ctrl+S shortcut
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // bridge.saveFile should be called with the correct path and content
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(path, content);
    });

    // After a successful save the dirty indicator must be cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain(path);
    });
  });

  it("shows 'Save failed' toast and keeps dirty indicator when bridge.saveFile rejects", async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      const path = '/project/test.ri';
      const content = 'module Test {}';

      vi.mocked(bridge.saveFile).mockRejectedValueOnce(new Error('disk full'));

      render(() => <App />);

      // Wait for App to be ready and capturedEditorStore to be set
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Open a file in the store, mark it dirty, and set as active
      capturedEditorStore.openFile({ path, content });
      capturedEditorStore.markDirty(path);
      capturedEditorStore.setActiveFile(path);

      // Confirm the file is dirty before the failing save
      expect(capturedEditorStore.state.dirtyFiles).toContain(path);

      // Trigger handleSave via global Ctrl+S shortcut
      fireEvent.keyDown(document, { key: 's', ctrlKey: true });

      // bridge.saveFile should be called with the correct path and content
      await waitFor(() => {
        expect(bridge.saveFile).toHaveBeenCalledWith(path, content);
      });

      // A toast containing "Save failed" and the error message must appear
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find(
          (t) => t.textContent?.includes('Save failed') && t.textContent?.includes('disk full'),
        );
        expect(errorToast).toBeTruthy();
      });

      // The dirty indicator must NOT be cleared (markClean was NOT called)
      expect(capturedEditorStore.state.dirtyFiles).toContain(path);

      // Targeted regression guard: handleSave must not regress into logging its failure
      // to console.error (the error must flow only through showToast). We do NOT use
      // `not.toHaveBeenCalled()` here because unrelated code paths (e.g. subscription
      // lifecycle, third-party libs) may legitimately emit console.error during this
      // test — this assertion tolerates that incidental noise while still flagging any
      // reintroduction of a `console.error('Save failed:', err)` call. Matches the
      // sibling pattern at setParameter (line ~1356) and re-evaluate (line ~1396).
      expect(errorSpy).not.toHaveBeenCalledWith('Save failed:', expect.any(Error));
    });
  });
});

describe('App tessellation diagnostics end-to-end wiring', () => {
  let tessellationDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    tessellationDiagnosticsCallback = undefined;
    vi.mocked(bridge.onTessellationDiagnostics).mockImplementation(async (cb: any) => {
      tessellationDiagnosticsCallback = cb;
      return () => {};
    });
  });

  it('tessellation-diagnostics event with one Error: StatusBar shows data-has-errors="true" and "Tessellation error"', async () => {
    render(() => <App />);
    // Wait until the app is ready and the callback has been registered
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    // Fire the tessellation-diagnostics event with one Error diagnostic
    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'geometry error: kernel failure',
        code: null,
      },
    ]);

    // Wait for the reactive update to propagate through engineStore → App → StatusBar
    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="tessellation-errors"]');
      expect(badge).toBeTruthy();
      expect(badge?.getAttribute('data-has-errors')).toBe('true');
      expect(statusBar.textContent).toMatch(/tessellation error/i);
    });
  });

  it('clicking tessellation-errors badge opens the diagnostics-panel containing the tessellation diagnostic', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: 'helper.ri',
        line: 5, column: 3, end_line: 5, end_column: 10,
        severity: 'Error',
        message: 'tess kernel boom',
        code: null,
      },
    ]);

    // Wait for the badge to appear
    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      expect(statusBar.querySelector('[data-testid="tessellation-errors"]')).toBeTruthy();
    });

    // Click the tessellation-errors badge
    fireEvent.click(screen.getByTestId('tessellation-errors'));

    // Panel should open and display the tessellation diagnostic message
    await waitFor(() => {
      const panel = screen.getByTestId('diagnostics-panel');
      expect(panel).toBeTruthy();
      expect(panel.textContent).toContain('tess kernel boom');
    });
  });

  it('clicking the tessellation-errors badge twice closes the panel', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'tess toggle close test',
        code: null,
      },
    ]);

    // Wait for the badge to appear
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // First click: open the panel
    fireEvent.click(screen.getByTestId('tessellation-errors'));
    await waitFor(() => expect(screen.getByTestId('diagnostics-panel')).toBeTruthy());

    // Second click: close the panel.
    // DiagnosticsPanel wraps its overlay in <Show when={props.open}>, so the
    // element is fully removed from the DOM (not just hidden) when closed.
    // This is the intentional contract: toBeNull() is the right assertion here;
    // a querySelector returning null proves the panel is unmounted, not merely
    // invisible — and would FAIL if the handler were flipped to setDiagnosticsOpen(true).
    fireEvent.click(screen.getByTestId('tessellation-errors'));
    await waitFor(() =>
      expect(document.querySelector('[data-testid="diagnostics-panel"]')).toBeNull()
    );
  });

  it('clicking a tessellation diagnostic row in the panel triggers setScrollToLocation', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: 'main.ri',
        line: 7, column: 4, end_line: 7, end_column: 9,
        severity: 'Error',
        message: 'tess nav test',
        code: null,
      },
    ]);

    // Wait for badge to appear then open the panel
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    fireEvent.click(screen.getByTestId('tessellation-errors'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
    });

    // Click the diagnostic row
    const row = document.querySelector('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row!);

    // scrollToLocation should be set to the tessellation diagnostic's location
    await waitFor(() => {
      const loc = capturedEditorScrollToLocation?.();
      expect(loc).toMatchObject({
        file_path: 'main.ri',
        line: 7,
        column: 4,
        end_line: 7,
        end_column: 9,
      });
    });
  });
});

describe('App compile diagnostics end-to-end wiring', () => {
  let compileDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    compileDiagnosticsCallback = undefined;
    vi.mocked((bridge as any).onCompileDiagnostics).mockImplementation(async (cb: any) => {
      compileDiagnosticsCallback = cb;
      return () => {};
    });
  });

  const warningDiag = {
    file_path: 'helper.ri',
    line: 3,
    column: 1,
    end_line: 3,
    end_column: 10,
    severity: 'Warning',
    message: "unknown port type 'Foo'",
    code: null,
  };

  it('compile-diagnostics event: diagnostics-count badge appears in StatusBar with "1 warning"', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="diagnostics-count"]');
      expect(badge).toBeTruthy();
      expect(badge!.textContent).toMatch(/1 warning/i);
    });
  });

  it('diagnostics-panel is NOT visible before clicking the badge', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    expect(document.querySelector('[data-testid="diagnostics-panel"]')).toBeNull();
  });

  it('clicking diagnostics-count badge opens the diagnostics-panel', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    fireEvent.click(screen.getByTestId('diagnostics-count'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
    });
  });

  it('clicking a diagnostic row triggers navigation via setScrollToLocation', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    // Open the panel
    fireEvent.click(screen.getByTestId('diagnostics-count'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
    });

    // Click the diagnostic row
    const row = document.querySelector('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row!);

    // Editor scrollToLocation should update with the diagnostic's location
    await waitFor(() => {
      const loc = capturedEditorScrollToLocation?.();
      expect(loc).toMatchObject({
        file_path: 'helper.ri',
        line: 3,
        column: 1,
      });
    });
  });

  it('pressing Escape inside the panel closes it', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    fireEvent.click(screen.getByTestId('diagnostics-count'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
    });

    // Fire Escape on document.body — matches user-observable behavior where
    // the overlay div is unfocused on open (document-level listener in
    // DiagnosticsPanel picks this up).
    fireEvent.keyDown(document.body, { key: 'Escape' });

    await waitFor(() => {
      expect(document.querySelector('[data-testid="diagnostics-panel"]')).toBeNull();
    });
  });

  it('compile-diagnostics event with Error severity: badge shows "1 error"', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    const errorDiag = {
      file_path: 'main.ri',
      line: 1,
      column: 1,
      end_line: 1,
      end_column: 5,
      severity: 'Error',
      message: 'import failed',
      code: null,
    };
    compileDiagnosticsCallback!([errorDiag]);

    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="diagnostics-count"]');
      expect(badge).toBeTruthy();
      expect(badge!.textContent).toMatch(/1 error/i);
    });
  });
});

describe('App merged diagnostics rendering', () => {
  let tessellationDiagnosticsCallback: ((diags: any[]) => void) | undefined;
  let compileDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    tessellationDiagnosticsCallback = undefined;
    compileDiagnosticsCallback = undefined;
    vi.mocked(bridge.onTessellationDiagnostics).mockImplementation(async (cb: any) => {
      tessellationDiagnosticsCallback = cb;
      return () => {};
    });
    vi.mocked((bridge as any).onCompileDiagnostics).mockImplementation(async (cb: any) => {
      compileDiagnosticsCallback = cb;
      return () => {};
    });
  });

  it('panel shows both compile and tessellation diagnostic messages when seeded simultaneously', async () => {
    render(() => <App />);
    await waitFor(() => {
      expect(tessellationDiagnosticsCallback).toBeDefined();
      expect(compileDiagnosticsCallback).toBeDefined();
    });

    compileDiagnosticsCallback!([
      {
        file_path: 'main.ri',
        line: 2, column: 1, end_line: 2, end_column: 5,
        severity: 'Warning',
        message: 'compile warn xyz',
        code: null,
      },
    ]);
    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'tess boom abc',
        code: null,
      },
    ]);

    // Wait for tessellation badge to appear
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // Open the panel via tessellation badge
    fireEvent.click(screen.getByTestId('tessellation-errors'));

    await waitFor(() => {
      const panel = screen.getByTestId('diagnostics-panel');
      expect(panel.textContent).toContain('compile warn xyz');
      expect(panel.textContent).toContain('tess boom abc');
    });
  });
});

describe('App Escape clears multi-selection', () => {
  it('pressing Escape after multi-selection empties selectedEntities', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    // Build up a multi-selection via Ctrl+clicks
    capturedDualViewportProps.onSelect('EntityA', { ctrl: true, shift: false });
    capturedDualViewportProps.onSelect('EntityB', { ctrl: true, shift: false });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA', 'EntityB']);
    });

    // Press Escape — should clear selection
    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual([]);
    });
  });

  it('pressing Escape after multi-selection also clears selectedEntity', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    capturedDualViewportProps.onSelect('EntityA', { ctrl: true, shift: false });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA']);
      expect(capturedDualViewportProps.selectedEntity).toBe('EntityA');
    });

    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual([]);
      expect(capturedDualViewportProps.selectedEntity).toBeNull();
    });
  });
});

describe('App viewport multi-selection modifier routing', () => {
  it('selectedEntities prop is passed from App to Viewport as an array', async () => {
    await renderAndWaitForReady();

    // The prop must exist and be an array (initially empty)
    expect(Array.isArray(capturedDualViewportProps.selectedEntities)).toBe(true);
    expect(capturedDualViewportProps.selectedEntities).toHaveLength(0);
  });

  it('Ctrl+click on viewport toggles entity into selectedEntities without calling navigateToSource', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    vi.mocked(bridge.getSourceLocation).mockClear();

    // Ctrl+click on EntityA: should call toggleSelect, adding it to selectedEntities
    capturedDualViewportProps.onSelect('EntityA', { ctrl: true, shift: false });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA']);
    });

    // getSourceLocation should NOT have been called for Ctrl+click
    expect(bridge.getSourceLocation).not.toHaveBeenCalled();
  });

  it('Ctrl+click multiple entities builds selectedEntities array', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    capturedDualViewportProps.onSelect('EntityA', { ctrl: true, shift: false });
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA']);
    });

    capturedDualViewportProps.onSelect('EntityB', { ctrl: true, shift: false });
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA', 'EntityB']);
    });
  });

  it('plain click on viewport calls navigateToSource (selectSingle) replacing prior multi-selection', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    // First, Ctrl+click to build a multi-selection
    capturedDualViewportProps.onSelect('EntityA', { ctrl: true, shift: false });
    capturedDualViewportProps.onSelect('EntityB', { ctrl: true, shift: false });
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityA', 'EntityB']);
    });

    vi.mocked(bridge.getSourceLocation).mockClear();

    // Plain click on EntityC: should call navigateToSource → selectSingle → replaces selection
    capturedDualViewportProps.onSelect('EntityC', { ctrl: false, shift: false });

    await waitFor(() => {
      expect(bridge.getSourceLocation).toHaveBeenCalledWith('EntityC');
    });

    // After selectSingle, selectedEntities = ['EntityC'] only
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toEqual(['EntityC']);
    });
  });

  it('backward compat: onSelect called with no modifiers still routes to navigateToSource', async () => {
    await renderAndWaitForReady();

    await waitFor(() => {
      expect(capturedDualViewportProps.onSelect).toBeDefined();
    });

    vi.mocked(bridge.getSourceLocation).mockClear();

    // Old-style call with no second arg (undefined modifiers)
    capturedDualViewportProps.onSelect('EntityD');

    await waitFor(() => {
      expect(bridge.getSourceLocation).toHaveBeenCalledWith('EntityD');
    });
  });
});

describe('App DesignTree wiring', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  it('renders DesignTree in the side panel', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      makeNode('Root.A'),
      makeNode('Root.B'),
    ]);
    await renderAndWaitForReady();
    expect(screen.getByTestId('design-tree')).toBeTruthy();
    expect(screen.getByTestId('tree-row-Root.A')).toBeTruthy();
    expect(screen.getByTestId('tree-row-Root.B')).toBeTruthy();
  });

  it('fetches entity tree on init', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId('tree-row-Root.A')).toBeTruthy();
  });

  it('re-fetches entity tree when evalStatus transitions from non-idle to idle', async () => {
    let evalStatusCallback: ((data: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(evalStatusCallback).toBeDefined());
    // Initial fetch on mount
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(1));

    // Transition non-idle → idle triggers a re-fetch
    evalStatusCallback!({ phase: 'evaluating' });
    evalStatusCallback!({ phase: 'idle' });
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(2));

    // Redundant idle → idle should NOT trigger another fetch
    evalStatusCallback!({ phase: 'idle' });
    // Give any potential spurious fetch a chance to fire
    await new Promise((r) => setTimeout(r, 50));
    expect(bridge.getEntityTree).toHaveBeenCalledTimes(2);
  });

  it('error phase → idle also triggers re-fetch', async () => {
    let evalStatusCallback: ((data: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(evalStatusCallback).toBeDefined());
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(1));

    // Transition error → idle should trigger a re-fetch (not just evaluating → idle)
    evalStatusCallback!({ phase: 'error' });
    evalStatusCallback!({ phase: 'idle' });
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(2));
  });

  it('initial idle phase does not cause spurious refetch beyond initApp fetch', async () => {
    // The phase-tracking createEffect must NOT fire a fetch on its first run when the
    // engine starts in idle — only initApp's explicit fetch should occur.
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    // Allow any spurious async effects to settle
    await new Promise((r) => setTimeout(r, 50));
    expect(bridge.getEntityTree).toHaveBeenCalledTimes(1);
  });

  it('plain click on a DesignTree row navigates to source and selects the entity', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    vi.mocked(bridge.getSourceLocation).mockResolvedValue({
      file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5,
    });
    await renderAndWaitForReady();

    fireEvent.click(screen.getByTestId('tree-row-Root.A'));

    await waitFor(() => expect(bridge.getSourceLocation).toHaveBeenCalledWith('Root.A'));
    expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-selected')).toBe('true');
    await waitFor(() => expect(capturedDualViewportProps.selectedEntity).toBe('Root.A'));
  });

  it('Ctrl+click on a DesignTree row toggles multi-selection without navigating to source', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A'), makeNode('Root.B')]);
    await renderAndWaitForReady();

    // Plain click on Root.A to seed selection and anchor
    fireEvent.click(screen.getByTestId('tree-row-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.selectedEntity).toBe('Root.A'));
    vi.mocked(bridge.getSourceLocation).mockClear();

    // Ctrl+click on Root.B to add to selection
    fireEvent.click(screen.getByTestId('tree-row-Root.B'), { ctrlKey: true });

    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntities).toContain('Root.A');
      expect(capturedDualViewportProps.selectedEntities).toContain('Root.B');
    });
    // getSourceLocation should NOT have been called for the Ctrl+click
    expect(bridge.getSourceLocation).not.toHaveBeenCalled();
  });

  it('Shift+click with an anchor performs a range-select', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      makeNode('Root.A'),
      makeNode('Root.B'),
      makeNode('Root.C'),
    ]);
    await renderAndWaitForReady();

    // Plain click Root.A to set anchor
    fireEvent.click(screen.getByTestId('tree-row-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.selectedEntity).toBe('Root.A'));
    vi.mocked(bridge.getSourceLocation).mockClear();

    // Shift+click Root.C to range-select A…C
    fireEvent.click(screen.getByTestId('tree-row-Root.C'), { shiftKey: true });

    await waitFor(() => {
      const sel = capturedDualViewportProps.selectedEntities as string[];
      expect(sel).toContain('Root.A');
      expect(sel).toContain('Root.B');
      expect(sel).toContain('Root.C');
    });
    // getSourceLocation should NOT have been called for the Shift+click (no source navigation)
    expect(bridge.getSourceLocation).not.toHaveBeenCalled();
  });

  it('Ctrl+A inside the tree selects all visible paths', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A'), makeNode('Root.B')]);
    await renderAndWaitForReady();

    fireEvent.keyDown(screen.getByTestId('design-tree'), { key: 'a', ctrlKey: true });

    await waitFor(() => {
      const sel = capturedDualViewportProps.selectedEntities as string[];
      expect(sel).toContain('Root.A');
      expect(sel).toContain('Root.B');
    });
    expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-selected')).toBe('true');
    expect(screen.getByTestId('tree-row-Root.B').getAttribute('data-selected')).toBe('true');
  });

  it('eye-icon click in DesignTree propagates to Viewport.entityVisibility', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();

    // Wait for tree to populate: structure node default is 'show'
    await waitFor(() => expect(capturedDualViewportProps.entityVisibility?.['Root.A']).toBe('show'));

    // First eye-icon click: show → ghost
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.entityVisibility?.['Root.A']).toBe('ghost'));

    // Second eye-icon click: ghost → hidden
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.entityVisibility?.['Root.A']).toBe('hidden'));
  });
});

// ---------------------------------------------------------------------------
// App — view selector and COW integration (step-27)
// ---------------------------------------------------------------------------

describe('App — view selector and COW integration', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  it('pressing "2" (no modifiers) switches to the second view in ViewSelector order', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    // Wait for ViewSelector trigger button showing the active view name "Default"
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    // Press "2" — second entry in selector order should be "All geometry"
    fireEvent.keyDown(document, { key: '2' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'All geometry' })).toBeTruthy());
  });

  it('clicking "Organize views…" opens ViewManageModal; Escape closes it', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    // Open ViewSelector dropdown
    fireEvent.click(screen.getByRole('button', { name: 'Default' }));
    // Click Organize views…
    fireEvent.click(screen.getByRole('menuitem', { name: /organize views/i }));
    // Modal opens
    expect(screen.getByRole('dialog')).toBeTruthy();
    // Escape on overlay closes it
    fireEvent.keyDown(screen.getByTestId('view-manage-overlay'), { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('COW user view appears in ViewSelector after eye-icon click and can be activated', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('eye-icon-Root.A')).toBeTruthy());
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    // Trigger COW: click eye-icon while "Default" auto-view is active
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    // ViewSelector button changes to "Default (modified)"
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Default (modified)' })).toBeTruthy()
    );
    // Switch back to Default using key "1" (first position = Default after sort)
    fireEvent.keyDown(document, { key: '1' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    // Open ViewSelector — "Default (modified)" user view should be listed
    fireEvent.click(screen.getByRole('button', { name: 'Default' }));
    expect(screen.getByRole('menuitem', { name: 'Default (modified)' })).toBeTruthy();
    // Click it to activate
    fireEvent.click(screen.getByRole('menuitem', { name: 'Default (modified)' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Default (modified)' })).toBeTruthy()
    );
  });

  it('COW: toggling eye-icon while auto view active creates "{autoName} (modified)" as active view', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('eye-icon-Root.A')).toBeTruthy());
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    // Click eye-icon (triggers COW since Default auto-view is active)
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
    // ViewSelector button now shows "Default (modified)"
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Default (modified)' })).toBeTruthy()
    );
  });
});

describe('Viewport wiring', () => {
  it('renders DualViewport (replaces bare Viewport)', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('dual-viewport')).toBeTruthy();
  });

  it('passes viewportStore to DualViewport', async () => {
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedDualViewportProps.viewportStore).toBeDefined());
  });
});

describe('Viewport view sync', () => {
  function makeNode(entity_path: string) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] };
  }

  it('initial viewId tracks viewStateStore.activeViewId ("auto:default")', async () => {
    await renderAndWaitForReady();
    const store = capturedDualViewportProps.viewportStore;
    // The createEffect should sync the initial activeViewId to the viewport immediately
    await waitFor(() => expect(store.getViewport('design-main').viewId).toBe('auto:default'));
  });

  it('viewId updates when a COW switch changes the active view', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    const store = capturedDualViewportProps.viewportStore;

    // Initial state: auto:default
    await waitFor(() => expect(store.getViewport('design-main').viewId).toBe('auto:default'));

    // Trigger a COW via eye-icon click — creates a new user view and switches to it
    await waitFor(() => expect(screen.getByTestId('eye-icon-Root.A')).toBeTruthy());
    fireEvent.click(screen.getByTestId('eye-icon-Root.A'));

    // The new user view should be the active view; viewId must reflect it
    await waitFor(() => {
      const viewId = store.getViewport('design-main').viewId;
      expect(viewId).not.toBeNull();
      expect(viewId).not.toBe('auto:default');
      expect(viewId).toMatch(/^user:/);
    });
  });
});

describe('DualViewport wiring', () => {
  it('App renders a DualViewport container', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('dual-viewport')).toBeTruthy();
  });

  it('App passes engineStore, defPreviewStore, and viewportStore props to DualViewport', async () => {
    await renderAndWaitForReady();
    await waitFor(() => {
      expect(capturedDualViewportProps.engineStore).toBeDefined();
      expect(capturedDualViewportProps.defPreviewStore).toBeDefined();
      expect(capturedDualViewportProps.viewportStore).toBeDefined();
    });
  });

  it('on cursor change, getContainingDefinition is called after 200ms debounce', async () => {
    // Render with real timers first so App init completes
    await renderAndWaitForReady();

    // Switch to fake timers AFTER App is mounted — the hook's createEffect will
    // schedule its setTimeout using the fake timer from this point on.
    // `vi.useFakeTimers()` is installed after mount, so only timers created
    // post-mount — the activation hook's 200ms debounce — are affected by
    // `advanceTimersByTimeAsync`. The `savePanelLayout` setTimeout at App.tsx:180
    // is scheduled with real timers during mount and is not owned by the fake
    // clock; prefer the isolated hook tests in useDefPreviewActivation.test.ts
    // for exhaustive debounce/race-condition coverage.
    vi.useFakeTimers();
    try {
      capturedEditorStore.setCursorPosition(5, 3);

      // Before debounce fires: bridge function should not be called yet
      expect(vi.mocked(bridge.getContainingDefinition)).not.toHaveBeenCalled();

      // Advance past the 200ms debounce window
      await vi.advanceTimersByTimeAsync(250);

      expect(vi.mocked(bridge.getContainingDefinition)).toHaveBeenCalledWith(5, 3);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ---------------------------------------------------------------------------
// Persistence wiring tests (steps 29–38)
// ---------------------------------------------------------------------------

/** Minimal valid PersistentViewState for test helpers. */
function makePersistedState(overrides: Partial<import('../types').PersistentViewState> = {}): import('../types').PersistentViewState {
  return {
    version: '2',
    activeViewId: 'user:my-view',
    userViews: [],
    explicit: {},
    viewportCameras: {},
    timestamp: '2026-01-01T00:00:00.000Z',
    ...overrides,
  };
}

describe('App persistence wiring — file open (step-29)', () => {
  it('on handleOpen success, loadSidecar(path) is queried first', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() => {
      expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith('/test/bracket.ri');
    });
  });

  it('when sidecar returns null, falls back to loadViewPersistence(path)', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue(null);

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() => {
      expect(viewPersistence.loadViewPersistence).toHaveBeenCalledWith('/test/bracket.ri');
    });
  });

  it('when sidecar returns a valid state, loadViewPersistence is NOT called', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue(makePersistedState());

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() => {
      expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith('/test/bracket.ri');
    });
    // Sidecar succeeded — localStorage should not be queried
    expect(viewPersistence.loadViewPersistence).not.toHaveBeenCalled();
  });

  it('when both layers return null, app renders without crash', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue(null);
    vi.mocked(viewPersistence.loadViewPersistence).mockReturnValue(null);

    render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    await waitFor(() => {
      expect(sidecarPersistence.loadSidecar).toHaveBeenCalled();
    });

    // App still renders correctly after null cascade
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// Step-31: debounced-save tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — debounced save (step-31)', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  /**
   * Flush all pending microtask queues from handleOpen's async chain.
   * handleOpen has ~6 awaits; each `await Promise.resolve()` flushes one
   * microtask level.  We call it 10× to be safe, without advancing fake timers.
   */
  async function flushHandleOpen() {
    for (let i = 0; i < 10; i++) {
      await Promise.resolve();
    }
  }

  it('no localStorage write happens before 500ms after a view-state mutation', async () => {
    // Render with real timers so App init completes, then switch to fake timers.
    // Only timers created AFTER the switch (i.e. the debounce timer) are faked.
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    await renderAndWaitForReady();

    vi.useFakeTimers();
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      // Flush handleOpen's await chain (Promise microtasks, not timer-based)
      await flushHandleOpen();

      // Advance 499ms — debounce threshold not yet reached
      await vi.advanceTimersByTimeAsync(499);

      expect(localStorage.getItem('reify:views:/test/bracket.ri')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('single localStorage write happens 500ms after the last mutation', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    await renderAndWaitForReady();

    vi.useFakeTimers();
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushHandleOpen();

      // Advance past the 500ms debounce window
      await vi.advanceTimersByTimeAsync(501);

      expect(localStorage.getItem('reify:views:/test/bracket.ri')).not.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('three rapid mutations within 500ms produce exactly one localStorage write', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    await renderAndWaitForReady();
    // Confirm eye icon is present before switching timers (init fetches entity tree)
    await waitFor(() => screen.getByTestId('eye-icon-Root.A'));

    vi.useFakeTimers();
    try {
      const setItemSpy = vi.spyOn(Storage.prototype, 'setItem');

      // Mutation 1: open file (currentFilePath changes → debounce schedule starts)
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushHandleOpen(); // let handleOpen complete; timer T1 set at t=0
      await vi.advanceTimersByTimeAsync(100); // t=100ms, T1 still has 400ms

      // Mutation 2: eye-icon click (viewStateStore.state changes → timer resets)
      fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
      await vi.advanceTimersByTimeAsync(100); // t=200ms, T2 set, 400ms remaining

      // Mutation 3: second eye-icon click
      fireEvent.click(screen.getByTestId('eye-icon-Root.A'));
      await vi.advanceTimersByTimeAsync(100); // t=300ms, T3 set, 400ms remaining

      // At t=699ms: 399ms elapsed since last mutation (300ms + 399ms advance)
      // 500ms timer has NOT fired (399 < 500)
      await vi.advanceTimersByTimeAsync(399); // t=699ms
      const writesBeforeWindow = setItemSpy.mock.calls.filter(
        ([k]) => k === 'reify:views:/test/bracket.ri',
      ).length;
      expect(writesBeforeWindow).toBe(0);

      // 1ms more → exactly 500ms since last mutation → debounce fires once
      await vi.advanceTimersByTimeAsync(1); // t=700ms
      const writesAfterWindow = setItemSpy.mock.calls.filter(
        ([k]) => k === 'reify:views:/test/bracket.ri',
      ).length;
      expect(writesAfterWindow).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('write key is reify:views:{path} and payload includes version and timestamp', async () => {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/bracket.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/bracket.ri', content: '' });
    await renderAndWaitForReady();

    vi.useFakeTimers();
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushHandleOpen();
      await vi.advanceTimersByTimeAsync(501);

      const raw = localStorage.getItem('reify:views:/test/bracket.ri');
      expect(raw).not.toBeNull();
      const parsed = JSON.parse(raw!);
      expect(parsed.version).toBe('2');
      expect(typeof parsed.timestamp).toBe('string');
    } finally {
      vi.useRealTimers();
    }
  });
});

// ---------------------------------------------------------------------------
// Step-33: Save views action tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — Save views action (step-33)', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  /** Open a file via Ctrl+O and wait for persistence to be queried. */
  async function openFile(path = '/test/bracket.ri') {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(path);
    vi.mocked(bridge.openFile).mockResolvedValue({ path, content: '' });
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await waitFor(() => expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith(path));
  }

  /** Open the ViewSelector dropdown by clicking the trigger button. */
  async function openViewSelectorDropdown() {
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Default' }));
  }

  it('(a) Save views button appears in ViewSelector dropdown when a file is open', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await openFile();

    await openViewSelectorDropdown();

    // "Save views" menuitem should appear in the dropdown
    await waitFor(() =>
      expect(screen.getByRole('menuitem', { name: /save views/i })).toBeTruthy()
    );
  });

  it('(b) clicking Save views calls saveSidecar(currentPath, composedState)', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await openFile();

    await openViewSelectorDropdown();
    fireEvent.click(screen.getByRole('menuitem', { name: /save views/i }));

    await waitFor(() => {
      expect(sidecarPersistence.saveSidecar).toHaveBeenCalledWith(
        '/test/bracket.ri',
        expect.objectContaining({ version: '2' }),
      );
    });
  });

  it('(c) on success, shows success toast containing sidecar filename', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    vi.mocked(sidecarPersistence.saveSidecar).mockResolvedValue(undefined);
    await renderAndWaitForReady();
    await openFile();

    await openViewSelectorDropdown();
    fireEvent.click(screen.getByRole('menuitem', { name: /save views/i }));

    // Toast should mention the sidecar filename
    await waitFor(() =>
      expect(screen.getByText(/bracket\.ri\.views\.json/)).toBeTruthy()
    );
  });

  it('(d) on saveSidecar rejection, shows error toast', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    vi.mocked(sidecarPersistence.saveSidecar).mockRejectedValue(new Error('disk full'));
    await renderAndWaitForReady();
    await openFile();

    await openViewSelectorDropdown();
    fireEvent.click(screen.getByRole('menuitem', { name: /save views/i }));

    await waitFor(() =>
      expect(screen.getByText(/disk full|failed.*save/i)).toBeTruthy()
    );
  });
});

// ---------------------------------------------------------------------------
// Step-37: Camera state restoration tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — camera state restoration (step-37)', () => {
  /** Open a file via Ctrl+O with a persisted sidecar containing camera state. */
  async function openFileWithCameras(
    path: string,
    cameras: Record<string, { position: [number, number, number]; target: [number, number, number]; up: [number, number, number]; zoom: number }>,
  ) {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(path);
    vi.mocked(bridge.openFile).mockResolvedValue({ path, content: '' });
    vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue({
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: cameras,
      timestamp: '2026-04-23T00:00:00.000Z',
    });

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    // Flush the async chain from handleOpen
    for (let i = 0; i < 10; i++) await Promise.resolve();
  }

  it('restores persisted camera for design-main viewport on openFile', async () => {
    const cam = { position: [3, 4, 5] as [number, number, number], target: [1, 2, 3] as [number, number, number], up: [0, 0, 1] as [number, number, number], zoom: 1.5 };

    await renderAndWaitForReady();
    await openFileWithCameras('/test/bracket.ri', { 'design-main': cam });

    // The viewportStore passed to DualViewport should reflect the restored camera
    await waitFor(() => {
      const vp = capturedDualViewportProps.viewportStore?.state.viewports['design-main'];
      expect(vp).toBeDefined();
      expect(vp.camera.position).toEqual([3, 4, 5]);
      expect(vp.camera.target).toEqual([1, 2, 3]);
      expect(vp.camera.up).toEqual([0, 0, 1]);
      expect(vp.camera.zoom).toBe(1.5);
    });
  });

  it('restores cameras for multiple viewports when sidecar contains them', async () => {
    const cams = {
      'design-main': { position: [10, 0, 0] as [number, number, number], target: [0, 0, 0] as [number, number, number], up: [0, 1, 0] as [number, number, number], zoom: 2 },
      'def-preview': { position: [0, 10, 0] as [number, number, number], target: [0, 0, 0] as [number, number, number], up: [0, 0, 1] as [number, number, number], zoom: 3 },
    };

    await renderAndWaitForReady();
    await openFileWithCameras('/test/bracket.ri', cams);

    await waitFor(() => {
      const viewports = capturedDualViewportProps.viewportStore?.state.viewports;
      expect(viewports?.['design-main'].camera.position).toEqual([10, 0, 0]);
      expect(viewports?.['def-preview'].camera.zoom).toBe(3);
    });
  });
});

// ---------------------------------------------------------------------------
// Step-35: Fuzzy-rebind notification tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — fuzzy-rebind notification (step-35)', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  /**
   * Capture the evalStatus callback so tests can drive phase transitions.
   * Must be called BEFORE renderAndWaitForReady().
   */
  function captureEvalStatus(): { get: () => ((data: any) => void) | undefined } {
    let cb: ((data: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (fn: any) => {
      cb = fn;
      return () => {};
    });
    return { get: () => cb };
  }

  /**
   * Drive an evaluating→idle transition and wait for the second getEntityTree call.
   */
  async function triggerTreeUpdate(
    evalCb: (data: any) => void,
    newTree: any[],
    expectedCallCount: number,
  ) {
    vi.mocked(bridge.getEntityTree).mockResolvedValueOnce(newTree);
    evalCb({ phase: 'evaluating' });
    evalCb({ phase: 'idle' });
    await waitFor(() =>
      expect(bridge.getEntityTree).toHaveBeenCalledTimes(expectedCallCount),
    );
  }

  it('(a) toast with Yes/No/Ignore buttons appears when stale path has a unique suffix candidate', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Set an explicit visibility on the path (click once: show → ghost)
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Trigger a tree update: Assembly.flange.geometry is gone; Assembly.bolt_flange.geometry appears
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    // A toast with [Yes][No][Ignore] buttons should appear
    await waitFor(() => screen.getByTestId('toast'));
    expect(screen.getByRole('button', { name: /^yes$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^no$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^ignore$/i })).toBeTruthy();
    // Toast message should reference the candidate or use "rebind"
    expect(screen.getByTestId('toast').textContent).toMatch(
      /Assembly\.bolt_flange\.geometry|Assembly\.flange\.geometry|rebind|rename/i,
    );
  });

  it('(b) clicking [Yes] dismisses the toast and transfers visibility to the new path', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Cycle Assembly.flange.geometry: show → ghost
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));
    expect(screen.getByTestId('eye-icon-Assembly.flange.geometry').getAttribute('aria-label')).toBe('ghost');

    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    await waitFor(() => screen.getByRole('button', { name: /^yes$/i }));
    fireEvent.click(screen.getByRole('button', { name: /^yes$/i }));

    // Toast should be dismissed
    await waitFor(() => expect(screen.queryByTestId('toast')).toBeNull());

    // New path should have the transferred visibility ('ghost')
    const newIcon = screen.getByTestId('eye-icon-Assembly.bolt_flange.geometry');
    expect(newIcon.getAttribute('aria-label')).toBe('ghost');
  });

  it('(c) clicking [No] dismisses the toast without transferring visibility', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Set explicit: show → ghost
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    await waitFor(() => screen.getByRole('button', { name: /^no$/i }));
    fireEvent.click(screen.getByRole('button', { name: /^no$/i }));

    // Toast dismissed
    await waitFor(() => expect(screen.queryByTestId('toast')).toBeNull());

    // New path should still have default visibility ('show') — no transfer
    const newIcon = screen.getByTestId('eye-icon-Assembly.bolt_flange.geometry');
    expect(newIcon.getAttribute('aria-label')).toBe('show');
  });

  it('(d) clicking [Ignore] prevents the same pair from triggering a toast on the next tree update', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    await waitFor(() => screen.getByRole('button', { name: /^ignore$/i }));
    fireEvent.click(screen.getByRole('button', { name: /^ignore$/i }));

    // Toast dismissed
    await waitFor(() => expect(screen.queryByTestId('toast')).toBeNull());

    // Trigger the same tree update again — same stale+candidate pair
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      3,
    );

    // No new toast should appear for the ignored pair
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('toast')).toBeNull();
  });

  it('(e) no toast appears when multiple candidates share the suffix of the stale path', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Two candidates for Assembly.flange.geometry — ambiguous, no toast expected
    await triggerTreeUpdate(
      evalRef.get()!,
      [
        makeNode('Assembly.bolt_flange.geometry'),
        makeNode('Assembly.hex_flange.geometry'),
      ],
      2,
    );

    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('toast')).toBeNull();
  });

  it('(f) clicking [No] suppresses the same stale→candidate pair on the next tree update', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Set explicit visibility: show → ghost
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // First tree update: Assembly.flange.geometry is gone; Assembly.bolt_flange.geometry appears
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    // Wait for toast and click [No]
    await waitFor(() => screen.getByRole('button', { name: /^no$/i }));
    fireEvent.click(screen.getByRole('button', { name: /^no$/i }));

    // Toast dismissed
    await waitFor(() => expect(screen.queryByTestId('toast')).toBeNull());

    // Trigger the SAME tree update again — same stale+candidate pair
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      3,
    );

    // No new toast should appear — [No] must suppress this pair for the session,
    // just like [Ignore] does. This is the regression the reviewer required.
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('toast')).toBeNull();
  });

  it('(g) does NOT enqueue a duplicate toast when a later tree update still reports the same stale pair', async () => {
    // Regression for reviewer blocker (gui/src/App.tsx:195-246):
    // If the user has not yet clicked Yes/No/Ignore, subsequent tree updates
    // that still leave the same stale→candidate pair outstanding must not
    // stack additional toasts for that pair.
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // Stamp an explicit visibility so the path becomes "stale" once it's
    // missing from the new tree.
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // First tree update — stale path appears with unique suffix candidate.
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );

    await waitFor(() => screen.getByTestId('toast'));
    expect(screen.getAllByTestId('toast').length).toBe(1);

    // Second tree update — same stale→candidate pair still outstanding.
    // Without the shown-pairs guard, this re-enters the rebind effect and
    // enqueues a second identical toast.
    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      3,
    );

    await new Promise((r) => setTimeout(r, 50));
    // Exactly one toast for this pair — no duplicate stacked.
    expect(screen.getAllByTestId('toast').length).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Review fix: flush debounced saver on path switch (gui/src/App.tsx:124-149)
// ---------------------------------------------------------------------------

describe('App persistence wiring — flush on file switch', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  async function flushHandleOpen() {
    for (let i = 0; i < 10; i++) {
      await Promise.resolve();
    }
  }

  it('opening a second file within the 500ms debounce window flushes the pending write for the first path', async () => {
    // Regression for reviewer blocker: previously the effect's onCleanup
    // called saver.cancel(), silently dropping the most recent mutation for
    // the outgoing file when the user switched files before the debounce
    // window expired.  The fix replaces cancel() with flush() keyed on path
    // transitions so the last state is persisted synchronously.
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);

    // First open: /test/first.ri
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/first.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/first.ri', content: '' });
    await renderAndWaitForReady();

    vi.useFakeTimers();
    try {
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushHandleOpen();

      // Advance only 100ms — well inside the 500ms debounce window; no write yet.
      await vi.advanceTimersByTimeAsync(100);
      expect(localStorage.getItem('reify:views:/test/first.ri')).toBeNull();

      // Switch to /test/second.ri while the timer is still pending.
      vi.mocked(bridge.pickOpenPath).mockResolvedValue('/test/second.ri');
      vi.mocked(bridge.openFile).mockResolvedValue({ path: '/test/second.ri', content: '' });
      fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
      await flushHandleOpen();

      // Path transition must flush the first file's pending state synchronously.
      expect(localStorage.getItem('reify:views:/test/first.ri')).not.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});

// ── MechanismPanel integration (step-21) ─────────────────────────────────────

describe('App MechanismPanel integration', () => {
  it('renders mechanism-panel inside the side panel', async () => {
    // Provide a mechanism descriptor so the panel is visible
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([
      {
        cell_id: 'Kinematic.m',
        entity_path: 'Kinematic',
        name: 'm',
        bodies_count: 2,
        joints: [
          {
            joint_index: 0,
            kind: 'prismatic',
            dimension: 'length',
            range_lower_si: 0.0,
            range_upper_si: 0.8,
            axis: [0, 1, 0],
            driving_param_cell_id: 'Kinematic.y_pos',
            current_value_si: 0.1,
            binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y_pos', current_value_si: 0.1 },
          },
        ],
      },
    ]);

    await renderAndWaitForReady();

    // MechanismPanel should be rendered in the side panel
    await waitFor(() => {
      expect(screen.getByTestId('mechanism-panel')).toBeTruthy();
    });
  });

  it('calls getMechanismDescriptors on non-idle to idle transition', async () => {
    let evalStatusCallback: ((data: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalStatusCallback).toBeDefined());

    const callsBefore = vi.mocked((bridge as any).getMechanismDescriptors).mock.calls.length;

    // Trigger a non-idle to idle transition
    evalStatusCallback!({ phase: 'evaluating' });
    evalStatusCallback!({ phase: 'idle' });

    await waitFor(() => {
      const callsAfter = vi.mocked((bridge as any).getMechanismDescriptors).mock.calls.length;
      expect(callsAfter).toBeGreaterThan(callsBefore);
    });
  });

  // Regression: when mechanism descriptors render alongside the chat panel, the
  // side-panel grid template had an extra 4px track before the mechanism row that
  // didn't correspond to any DOM splitter. Children after ConstraintPanel (mech,
  // splitter-constraint, chat) all shifted up one track, collapsing the chat
  // container into a 4px slot and clipping its content under overflow:hidden.
  // Assert track count == direct-child count for the affected configurations.
  it('side-panel grid track count matches child count with mechanism + chat', async () => {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([
      {
        cell_id: 'Kinematic.m', entity_path: 'Kinematic', name: 'm', bodies_count: 2,
        joints: [{
          joint_index: 0, kind: 'prismatic', dimension: 'length',
          range_lower_si: 0, range_upper_si: 0.8, axis: [0, 1, 0],
          driving_param_cell_id: 'Kinematic.y', current_value_si: 0.1,
          binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y', current_value_si: 0.1 },
        }],
      },
    ]);

    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('mechanism-panel')).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId('chat-panel')).toBeTruthy());

    const sidePanel = screen.getByTestId('side-panel') as HTMLElement;
    const tracks = countGridTracks(sidePanel.style.gridTemplateRows);
    const children = sidePanel.children.length;
    expect(tracks).toBe(children);
  });

  it('side-panel grid track count matches child count with mechanism + no chat', async () => {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([
      {
        cell_id: 'Kinematic.m', entity_path: 'Kinematic', name: 'm', bodies_count: 2,
        joints: [{
          joint_index: 0, kind: 'prismatic', dimension: 'length',
          range_lower_si: 0, range_upper_si: 0.8, axis: [0, 1, 0],
          driving_param_cell_id: 'Kinematic.y', current_value_si: 0.1,
          binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y', current_value_si: 0.1 },
        }],
      },
    ]);

    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('mechanism-panel')).toBeTruthy());

    // Toggle chat off via the StatusBar control (the only user path).
    const sidePanel = screen.getByTestId('side-panel') as HTMLElement;
    // Use the chat-toggle control surfaced by StatusBar; falls back to keyboard
    // shortcut if exposed differently — current StatusBar uses claude-status as
    // the toggle target via onToggleChat. Click the claude-status span.
    const claudeStatus = screen.getByTestId('claude-status') as HTMLElement;
    claudeStatus.click();

    await waitFor(() => expect(screen.queryByTestId('chat-panel')).toBeNull());

    const tracks = countGridTracks(sidePanel.style.gridTemplateRows);
    const children = sidePanel.children.length;
    expect(tracks).toBe(children);
  });
});

// ─── externallyChanged wiring in App.tsx ─────────────────────────────────────

describe('App externallyChanged store wiring', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    vi.mocked(bridge.openFile).mockImplementation(async (path: string) => ({
      path,
      content: `updated ${path}`,
    }));
  });

  it('(a) onFileChanged for an open file adds it to externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty for the externally-changed path to trigger (non-dirty auto-reloads silently)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');
    });
  });

  it('(b) onFileChanged for a path NOT in openFiles does NOT add to externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // '/project/other.ri' is not in the initial state files
    fileChangedCallback!({ path: '/project/other.ri', content: 'new' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 10));
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/other.ri');
  });

  it('(c) after handleReload succeeds, path is removed from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // The file is dirty, so clicking Reload first shows the "Reload Anyway" confirmation.
    await waitFor(() => expect(screen.getByText('Reload')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload'));

    // After the dirty-overlap confirmation appears, clicking "Reload Anyway" proceeds.
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload Anyway'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });
  });

  it('(c2) after handleReload succeeds, dirtyFiles is also cleared when the file had unsaved edits', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Simulate: user had typed unsaved edits AND an external file-changed event arrives
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'disk content' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );
    expect(capturedEditorStore.state.dirtyFiles).toContain('/project/bracket.ri');

    // First Reload click: dirty overlap detected → shows "Reload Anyway" confirmation
    await waitFor(() => expect(screen.getByText('Reload')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload'));

    // Second click on "Reload Anyway" → proceeds with actual reload
    await waitFor(() => expect(screen.getByText('Reload Anyway')).toBeTruthy());
    fireEvent.click(screen.getByText('Reload Anyway'));

    // After reload both flags are cleared: the buffer was replaced with disk
    // content so neither user-edits nor disk-divergence remain.
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
    });
    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });
  });

  it('(d) handleDismissReload clears all paths from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // Click Dismiss — independent of dirty state, clears all changedFiles + externallyChanged
    await waitFor(() => expect(screen.getByText('Dismiss')).toBeTruthy());
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    });
  });

  it('(e) handleDismissReload with multiple paths clears all entries from externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Must be dirty so the event triggers the externally-changed path (not silent auto-reload)
    capturedEditorStore.markDirty('/project/bracket.ri');
    // Trigger changedFiles for bracket.ri so the Dismiss button appears
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'new' });
    await waitFor(() =>
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri'),
    );

    // Add two more paths directly (simulates concurrent disk changes to other open files)
    capturedEditorStore.markExternallyChanged('/project/b.ri');
    capturedEditorStore.markExternallyChanged('/project/c.ri');
    expect(capturedEditorStore.state.externallyChanged.length).toBe(3);
    expect(capturedEditorStore.state.externallyChanged).toEqual(
      expect.arrayContaining(['/project/bracket.ri', '/project/b.ri', '/project/c.ri']),
    );

    // Click Dismiss — should atomically clear all three
    await waitFor(() => expect(screen.getByText('Dismiss')).toBeTruthy());
    fireEvent.click(screen.getByText('Dismiss'));

    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    });
  });
});

// ─── Auto-reload non-dirty tabs on file-changed event ────────────────────────

describe('App file-changed auto-reload (non-dirty)', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('(a) file-changed for a non-dirty open tab silently updates content without setting externallyChanged', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // File is NOT dirty (no markDirty call)
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'auto-reloaded content' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 20));

    // Content should be updated in the store
    const file = capturedEditorStore.state.openFiles.find(
      (f: any) => f.path === '/project/bracket.ri',
    );
    expect(file?.content).toBe('auto-reloaded content');

    // externallyChanged should remain empty — no conflict to surface
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');

    // No reload-prompt banner should be visible (changedFiles is empty)
    expect(screen.queryByText('Reload')).toBeNull();

    // The auto-reload must NOT mark the file dirty — markClean runs after updateFileContent,
    // so dirtyFiles should not contain this path.
    expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');

    // The auto-reload must NOT echo back to the backend as a phantom user edit.
    // (Editor is mocked in App tests, so no CodeMirror view, but this is a guard for
    // the future when App tests may use the real Editor.)
    expect(bridge.updateSource).not.toHaveBeenCalled();
  });

  it('(b) file-changed for a DIRTY open tab still triggers externallyChanged and shows the Reload banner', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark the file as dirty (user has unsaved edits)
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'disk changed' });

    // Dirty path: externallyChanged should be set and the banner rendered
    await waitFor(() => {
      expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');
    });
    await waitFor(() => {
      expect(screen.getByText('Reload')).toBeTruthy();
    });
  });

  it('(c) file-changed for a path NOT in openFiles does nothing', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileChangedCallback!({ path: '/project/unknown.ri', content: 'some content' });

    await new Promise((r) => setTimeout(r, 20));

    // No state mutation, no banner, no toast
    expect(capturedEditorStore.state.externallyChanged).toEqual([]);
    expect(screen.queryByText('Reload')).toBeNull();
  });
});

// ─── isSameFile cross-format path matching in onFileChanged ─────────────────

describe('App file-changed isSameFile cross-format matching', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/foo.ri', content: 'old content' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('file-changed with file:// URI matches a bare-path tab and auto-reloads its content', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Tab has bare path '/project/foo.ri'; event arrives with file:// URI.
    // The handler must use isSameFile to match them.
    fileChangedCallback!({ path: 'file:///project/foo.ri', content: 'NEW' });

    // Give reactivity a chance to settle
    await new Promise((r) => setTimeout(r, 20));

    // Content should be updated — the URI matched the bare-path tab
    const file = capturedEditorStore.state.openFiles.find(
      (f: any) => f.path === '/project/foo.ri',
    );
    expect(file?.content).toBe('NEW');

    // No conflict UI — non-dirty auto-reload
    expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/foo.ri');
    expect(screen.queryByText('Reload')).toBeNull();
  });
});

// ─── handleSave aborts when file is externally changed ───────────────────────

describe('App handleSave aborts when file is externally changed', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('(a) handleSave does NOT call bridgeSaveFile and shows conflict prompt when active file is externally changed', async () => {
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    // Regression guard: neither handleSave nor the Editor keymap should emit
    // console.error for the externally-changed branch (only for not-found).
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      render(() => <App />);
      await waitFor(() => expect(fileChangedCallback).toBeDefined());
      await waitFor(() => expect(capturedEditorStore).toBeTruthy());

      // Set active file
      capturedEditorStore.setActiveFile('/project/bracket.ri');

      // Mark the file as externally changed
      capturedEditorStore.markExternallyChanged('/project/bracket.ri');

      // Trigger handleSave via Ctrl+S
      fireEvent.keyDown(document, { key: 's', ctrlKey: true });

      // Wait a beat for any async effects
      await new Promise((r) => setTimeout(r, 20));

      // bridgeSaveFile (bridge.saveFile) must NOT have been called
      expect(bridge.saveFile).not.toHaveBeenCalled();

      // Neither handleSave nor the Editor keymap should emit 'Save aborted' for
      // the externally-changed branch (only for not-found). We narrow the check
      // to the specific message substring so that unrelated SolidJS / dev-mode
      // console.error calls don't cause spurious failures.
      expect(
        consoleErrorSpy.mock.calls
          .flat()
          .some((arg: unknown) => typeof arg === 'string' && arg.includes('Save aborted')),
      ).toBe(false);

      // A conflict toast (not a dead-end error) must appear with the prompt message
      // and both action buttons. Using the full constant (not a loose keyword regex)
      // enforces lockstep: a wording drift in messages.ts would break this test.
      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const conflictToast = toasts.find((t) =>
          t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
        );
        expect(conflictToast).toBeTruthy();
        // Must be 'error' type (sticky, no auto-dismiss) — matches the old blocked toast
        expect(conflictToast?.dataset.type).toBe('error');
        // Both action buttons must be present within the toast
        expect(within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL })).toBeTruthy();
        expect(within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL })).toBeTruthy();
      });
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it('(b) after clearExternallyChanged, handleSave DOES call bridgeSaveFile', async () => {
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');
    // Clear the external-change flag — save should now proceed
    capturedEditorStore.clearExternallyChanged('/project/bracket.ri');

    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(
        '/project/bracket.ri',
        expect.any(String),
      );
    });
  });

  it('(c) two Ctrl+S in a row while externally changed produce only one conflict prompt', async () => {
    // Guards against stacked prompts: if the user reflexively presses Ctrl+S twice,
    // showSaveConflictPrompt must deduplicate so only one toast is visible.  A stacked
    // second prompt could silently discard newer edits if the user clicks Reload in
    // the older copy after having typed more into the buffer.
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');

    // Press Ctrl+S twice in quick succession
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    await new Promise((r) => setTimeout(r, 20));

    // Exactly ONE conflict toast must be visible — not two stacked copies
    const conflictToasts = screen
      .getAllByTestId('toast')
      .filter((t) => t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG));
    expect(conflictToasts).toHaveLength(1);
  });
});


// ─── Conflict prompt: Reload from disk action ────────────────────────────────

describe('App handleSave conflict prompt: Reload from disk', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('clicking Reload from disk calls bridgeOpenFile, updates content, and clears both dirty/externallyChanged flags', async () => {
    const diskContent = 'structure Bracket { /* updated on disk */ }';
    vi.mocked(bridge.openFile).mockResolvedValue({
      path: '/project/bracket.ri',
      content: diskContent,
    });
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Set up dirty + externally-changed state
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');

    // Trigger handleSave via Ctrl+S — should show conflict prompt (not blocked msg)
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Wait for the conflict toast with Reload action to appear
    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click the "Reload from disk" action button
    const reloadBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL });
    fireEvent.click(reloadBtn);

    // bridgeOpenFile must be called with the file's path
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });

    // After the reload promise resolves, the store's content is the new disk content
    await waitFor(() => {
      const file = capturedEditorStore.state.openFiles.find(
        (f: any) => f.path === '/project/bracket.ri',
      );
      expect(file?.content).toBe(diskContent);
    });

    // markClean was called — both dirtyFiles and externallyChanged are cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // bridgeSaveFile must NOT have been called
    expect(bridge.saveFile).not.toHaveBeenCalled();
  });
});

// ─── Conflict prompt: Overwrite action ───────────────────────────────────────

describe('App handleSave conflict prompt: Overwrite', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('clicking Overwrite calls bridgeSaveFile with the buffer content and clears dirty/externallyChanged without calling bridgeOpenFile', async () => {
    const bufferContent = 'structure Bracket { param width = 100mm }'; // user's edited content
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/bracket.ri', content: 'unused' });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Set up dirty + externally-changed state with custom buffer content
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    capturedEditorStore.markExternallyChanged('/project/bracket.ri');
    // Update the store's buffer content to simulate user edits
    capturedEditorStore.updateFileContent('/project/bracket.ri', bufferContent);

    // Trigger handleSave via Ctrl+S — shows conflict prompt
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    // Wait for the conflict toast with Overwrite button to appear
    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click the "Overwrite" action button
    const overwriteBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL });
    fireEvent.click(overwriteBtn);

    // bridgeSaveFile must be called with (path, currentBufferContent)
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith(
        '/project/bracket.ri',
        bufferContent,
      );
    });

    // After save promise resolves, markClean was called — both flags cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // bridgeOpenFile must NOT have been called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

// ─── Save-conflict resolution clears the reload-prompt banner ────────────────

describe('App save-conflict resolution clears the reload-prompt banner', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileChangedCallback: ((data: { path: string; content: string }) => void) | undefined;

  beforeEach(() => {
    fileChangedCallback = undefined;
    vi.mocked(bridge.onFileChanged).mockImplementation(async (cb: any) => {
      fileChangedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('(a) Reload from disk action clears the changedFiles banner after resolving the conflict', async () => {
    // Setup: dirty file + file-changed event → changedFiles + externallyChanged both set.
    // Guards: banner must appear, conflict prompt must surface via Ctrl+S,
    // then clicking Reload from disk must clear the banner (changedFiles cleared).
    const diskContent = '/* updated on disk */';
    vi.mocked(bridge.openFile).mockResolvedValue({
      path: '/project/bracket.ri',
      content: diskContent,
    });
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark dirty first so the onFileChanged handler takes the dirty branch
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');

    // Fire file-changed: dirty branch → adds to changedFiles AND calls markExternallyChanged
    fileChangedCallback!({ path: '/project/bracket.ri', content: diskContent });

    // Banner must become visible (changedFiles is non-empty)
    await waitFor(() => {
      // data-testid="reload-prompt" is rendered by ReloadPrompt when filePaths.length > 0
      expect(screen.queryByTestId('reload-prompt')).not.toBeNull();
    });

    // externallyChanged must also be set (both signals populated by the dirty branch)
    expect(capturedEditorStore.state.externallyChanged).toContain('/project/bracket.ri');

    // Trigger handleSave via Ctrl+S → conflict prompt appears (externallyChanged is set)
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click "Reload from disk" → reloadFromDisk: bridgeOpenFile + updateFileContent + markClean
    const reloadBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_RELOAD_LABEL });
    fireEvent.click(reloadBtn);

    // bridgeOpenFile was called with the correct path
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    });

    // After reloadFromDisk resolves: the banner must be gone (changedFiles cleared).
    // This guards against banner staleness: reloadFromDisk must also call
    // setChangedFiles((prev) => { next.delete(path); return next; }) in addition to markClean.
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // Store state is also clear
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // Overwrite was NOT triggered
    expect(bridge.saveFile).not.toHaveBeenCalled();
  });

  it('(b) Overwrite action clears the changedFiles banner after resolving the conflict', async () => {
    // Same setup as (a) but clicks Overwrite — bridgeSaveFile must be called and
    // the banner must disappear (changedFiles cleared), bridgeOpenFile NOT called.
    const bufferContent = 'structure Bracket { param width = 100mm }';
    vi.mocked(bridge.saveFile).mockResolvedValue(undefined);
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/bracket.ri', content: 'unused' });

    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Mark dirty, fire file-changed → changedFiles + externallyChanged
    capturedEditorStore.setActiveFile('/project/bracket.ri');
    capturedEditorStore.markDirty('/project/bracket.ri');
    fileChangedCallback!({ path: '/project/bracket.ri', content: 'newer disk content' });

    // Banner appears
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).not.toBeNull();
    });

    // Set the buffer content that the user edited
    capturedEditorStore.updateFileContent('/project/bracket.ri', bufferContent);

    // Ctrl+S → conflict prompt
    fireEvent.keyDown(document, { key: 's', ctrlKey: true });

    let conflictToast: HTMLElement | undefined;
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      conflictToast = toasts.find((t) =>
        t.textContent?.includes(EXTERNALLY_CHANGED_SAVE_CONFLICT_PROMPT_MSG),
      );
      expect(conflictToast).toBeTruthy();
    });

    // Click "Overwrite"
    const overwriteBtn = within(conflictToast!).getByRole('button', { name: SAVE_CONFLICT_OVERWRITE_LABEL });
    fireEvent.click(overwriteBtn);

    // bridgeSaveFile called with the buffer content
    await waitFor(() => {
      expect(bridge.saveFile).toHaveBeenCalledWith('/project/bracket.ri', bufferContent);
    });

    // After overwriteFile resolves: banner must be gone (changedFiles cleared).
    // Guards against banner staleness: overwriteFile must also call
    // setChangedFiles((prev) => { next.delete(path); return next; }) in addition to markClean.
    await waitFor(() => {
      expect(screen.queryByTestId('reload-prompt')).toBeNull();
    });

    // Store state cleared
    await waitFor(() => {
      expect(capturedEditorStore.state.dirtyFiles).not.toContain('/project/bracket.ri');
      expect(capturedEditorStore.state.externallyChanged).not.toContain('/project/bracket.ri');
    });

    // Reload NOT called
    expect(bridge.openFile).not.toHaveBeenCalled();
  });
});

// Counts CSS grid tracks in a `grid-template-rows` value.
// Whitespace separates tracks at depth 0; parens (e.g. minmax(160px, 1fr))
// keep their internal whitespace from being mistaken for a track boundary.
function countGridTracks(template: string): number {
  let depth = 0;
  let inTrack = false;
  let count = 0;
  for (const ch of template) {
    if (ch === '(') { depth++; inTrack = true; }
    else if (ch === ')') { depth--; inTrack = true; }
    else if (depth === 0 && /\s/.test(ch)) {
      if (inTrack) { count++; inTrack = false; }
    } else {
      inTrack = true;
    }
  }
  if (inTrack) count++;
  return count;
}

// ── AutoResolvePanel integration (step-13) ────────────────────────────────────

describe('App AutoResolvePanel integration', () => {
  it('AutoResolvePanel auto-promotes when state.autoResolve.active is true', async () => {
    // Capture the auto-resolve bridge callbacks registered by engineStore.subscribeToEvents
    let startCb: (() => void) | undefined;
    let completeCb: (() => void) | undefined;
    vi.mocked((bridge as any).onAutoResolveStart).mockImplementation(async (cb: () => void) => {
      startCb = cb;
      return () => {};
    });
    vi.mocked((bridge as any).onAutoResolveComplete).mockImplementation(async (cb: () => void) => {
      completeCb = cb;
      return () => {};
    });

    await renderAndWaitForReady();

    // Wait for subscribeToEvents to register the callbacks
    await waitFor(() => expect(startCb).toBeDefined());
    await waitFor(() => expect(completeCb).toBeDefined());

    // Panel should NOT be visible before any loop starts
    expect(screen.queryByTestId('auto-resolve-panel')).toBeNull();

    // Fire the auto-resolve-start event — panel should auto-promote
    startCb!();
    await waitFor(() => {
      expect(screen.queryByTestId('auto-resolve-panel')).toBeTruthy();
    });

    // Fire the auto-resolve-complete event — panel should be hidden again
    completeCb!();
    await waitFor(() => {
      expect(screen.queryByTestId('auto-resolve-panel')).toBeNull();
    });
  });
});

describe('App editor→selection wiring', () => {
  it('(a) App invokes getEntityAtSourceLocation after editor cursor changes and updates selectionStore.selectedEntity', async () => {
    vi.mocked((bridge as any).getEntityAtSourceLocation).mockResolvedValue('Bracket.width');

    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    vi.useFakeTimers();
    try {
      capturedEditorStore.setCursorPosition(2, 11);
      await vi.advanceTimersByTimeAsync(250);
      // Let any microtasks (Promise resolutions) settle
      await Promise.resolve();
      await Promise.resolve();

      await waitFor(() => {
        expect(vi.mocked((bridge as any).getEntityAtSourceLocation)).toHaveBeenCalledWith(2, 11);
        expect(capturedDualViewportProps.selectedEntity).toBe('Bracket.width');
        expect(mockFlyToEntity).toHaveBeenCalledWith('Bracket.width');
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('(b) App does NOT clear selection when getEntityAtSourceLocation returns null', async () => {
    // Pre-select via viewport click
    vi.mocked(bridge.getSourceLocation).mockResolvedValue({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 });
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedDualViewportProps.onSelect).toBeDefined());

    capturedDualViewportProps.onSelect('Bracket');
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntity).toBe('Bracket');
    });

    // Now set bridge to return null and move cursor
    vi.mocked((bridge as any).getEntityAtSourceLocation).mockResolvedValue(null);

    vi.useFakeTimers();
    try {
      capturedEditorStore.setCursorPosition(1, 1);
      await vi.advanceTimersByTimeAsync(250);
      await Promise.resolve();
      await Promise.resolve();

      // Selection must NOT have been cleared
      expect(capturedDualViewportProps.selectedEntity).toBe('Bracket');
    } finally {
      vi.useRealTimers();
    }
  });

  it('(c) App skips selectEntity and flyTo when getEntityAtSourceLocation returns the currently-selected entity', async () => {
    // Pre-select 'Bracket.width' via viewport click
    vi.mocked(bridge.getSourceLocation).mockResolvedValue({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 });
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedDualViewportProps.onSelect).toBeDefined());

    capturedDualViewportProps.onSelect('Bracket.width');
    await waitFor(() => {
      expect(capturedDualViewportProps.selectedEntity).toBe('Bracket.width');
    });

    mockFlyToEntity.mockClear();

    // Bridge returns the same entity
    vi.mocked((bridge as any).getEntityAtSourceLocation).mockResolvedValue('Bracket.width');

    vi.useFakeTimers();
    try {
      capturedEditorStore.setCursorPosition(2, 11);
      await vi.advanceTimersByTimeAsync(250);
      await Promise.resolve();
      await Promise.resolve();

      // Equality-check guard: flyToEntity must NOT be called again (no bounce)
      expect(mockFlyToEntity).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});

// ─── file-removed event handling (step-23) ───────────────────────────────────

describe('App file-removed event handling', () => {
  const testState: GuiState = {
    meshes: [],
    values: [],
    constraints: [],
    files: [
      { path: '/project/bracket.ri', content: 'structure Bracket {}' },
    ],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
  };

  let fileRemovedCallback: ((data: { path: string }) => void) | undefined;

  beforeEach(() => {
    fileRemovedCallback = undefined;
    vi.mocked((bridge as any).onFileRemoved).mockImplementation(async (cb: any) => {
      fileRemovedCallback = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
  });

  it('(a) onFileRemoved is subscribed to during initApp', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    expect(bridge.onFileRemoved).toHaveBeenCalled();
  });

  it('(b) firing with a path in openFiles adds it to missingFiles', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileRemovedCallback!({ path: '/project/bracket.ri' });

    await waitFor(() => {
      expect(capturedEditorStore.state.missingFiles).toContain('/project/bracket.ri');
    });
  });

  it('(c) firing with a path NOT in openFiles does NOT add to missingFiles', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileRemovedCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    fileRemovedCallback!({ path: '/project/other.ri' });

    await new Promise((r) => setTimeout(r, 10));
    expect(capturedEditorStore.state.missingFiles).not.toContain('/project/other.ri');
  });

  it('(d) unsub from onFileRemoved is called on App unmount', async () => {
    const fileRemovedUnsub = vi.fn();
    vi.mocked((bridge as any).onFileRemoved).mockResolvedValue(fileRemovedUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());

    unmount();
    expect(fileRemovedUnsub).toHaveBeenCalled();
  });
});

// ── SolverProgressOverlay integration (step-15) ────────────────────────────

describe('App SolverProgressOverlay integration', () => {
  it('(a) solver-progress-overlay is absent by default', async () => {
    await renderAndWaitForReady();
    expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();
  });

  it('(b) solver-progress-overlay renders after >1s of solver-progress ticks', async () => {
    let progressCb: ((p: any) => void) | undefined;
    vi.mocked((bridge as any).onSolverProgress).mockImplementation(
      async (cb: (p: any) => void) => {
        progressCb = cb;
        return () => {};
      },
    );

    // Render with real timers so App init (waitFor) works
    await renderAndWaitForReady();
    await waitFor(() => expect(progressCb).toBeDefined());

    // Overlay absent before any tick
    expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();

    // Switch to fake timers so the debounce setTimeout uses the fake clock
    vi.useFakeTimers();
    try {
      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });

      // Still absent — debounce has not expired yet
      expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();

      // Advance past the 1000ms debounce
      await vi.advanceTimersByTimeAsync(1000);

      expect(screen.queryByTestId('solver-progress-overlay')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('(c) clicking Cancel in overlay invokes bridge cancelSolve', async () => {
    let progressCb: ((p: any) => void) | undefined;
    vi.mocked((bridge as any).onSolverProgress).mockImplementation(
      async (cb: (p: any) => void) => {
        progressCb = cb;
        return () => {};
      },
    );

    await renderAndWaitForReady();
    await waitFor(() => expect(progressCb).toBeDefined());

    vi.useFakeTimers();
    try {
      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });
      await vi.advanceTimersByTimeAsync(1000);

      expect(screen.queryByTestId('solver-progress-overlay')).toBeTruthy();

      fireEvent.click(screen.getByText('Cancel'));

      // cancelSolve is async — flush microtasks
      await Promise.resolve();
      await Promise.resolve();

      expect(vi.mocked((bridge as any).cancelSolve)).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
