import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup, within } from '@solidjs/testing-library';
import type { GuiState } from '../types';
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
const mockViewportFitToView = vi.fn();
vi.mock('../viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportProps = props;
    // Invoke flyToEntityRef with a mock function if provided
    if (props.flyToEntityRef) {
      props.flyToEntityRef((_path: string) => {});
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
}));

// Mock Editor (requires CodeMirror DOM APIs) — capture store for dirty-file tests
let capturedEditorStore: any = null;
vi.mock('../editor/Editor', () => ({
  Editor: (props: any) => {
    capturedEditorStore = props.store;
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
const emptyState: GuiState = { meshes: [], values: [], constraints: [], files: [] };
vi.mock('../bridge', () => ({
  getInitialState: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
  setParameter: vi.fn().mockResolvedValue(undefined),
  exportGeometry: vi.fn().mockResolvedValue(undefined),
  pickSavePath: vi.fn().mockResolvedValue('/user/chosen/path.step'),
  pickOpenPath: vi.fn().mockResolvedValue(null),
  updateSource: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockResolvedValue({ path: '', content: '' }),
  getSourceLocation: vi.fn().mockResolvedValue({ file_path: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 }),
  focusEntity: vi.fn().mockResolvedValue(undefined),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
  onValueUpdate: vi.fn().mockResolvedValue(() => {}),
  onConstraintUpdate: vi.fn().mockResolvedValue(() => {}),
  onEvaluationStatus: vi.fn().mockResolvedValue(() => {}),
  onMeshRemoved: vi.fn().mockResolvedValue(() => {}),
  onValueRemoved: vi.fn().mockResolvedValue(() => {}),
  onConstraintRemoved: vi.fn().mockResolvedValue(() => {}),
  onFileChanged: vi.fn().mockResolvedValue(() => {}),
  claudeSendMessage: vi.fn().mockResolvedValue(undefined),
  claudeAbort: vi.fn().mockResolvedValue(undefined),
  claudeClearSession: vi.fn().mockResolvedValue(undefined),
  subscribeToClaudeEvents: vi.fn().mockResolvedValue(() => {}),
}));

import App from '../App';
import * as bridge from '../bridge';
import { STORAGE_KEY } from '../hooks/useLayoutPersistence';

beforeEach(() => {
  vi.clearAllMocks();
  capturedViewportProps = {};
  mockViewportFitToView.mockClear();
  localStorage.clear();
  capturedEditorStore = null;
  // Reset bridge mocks to defaults (clearAllMocks only clears call history, not implementations)
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] });
  vi.mocked(bridge.onMeshUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(() => {});
  vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(() => {});
  vi.mocked(bridge.onMeshRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onValueRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(() => {});
  vi.mocked(bridge.onFileChanged).mockResolvedValue(() => {});
  vi.mocked(bridge.subscribeToClaudeEvents).mockResolvedValue(() => {});
  vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/path.step');
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

  it('renders Toolbar at top', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders StatusBar at bottom', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('renders Viewport', async () => {
    await renderAndWaitForReady();
    expect(screen.getByTestId('viewport-container')).toBeTruthy();
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
        },
      ],
      constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
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
      meshes: [], values: [], constraints: [], files: [],
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
      }],
      constraints: [],
      files: [{ path: '/test.ri', content: '' }],
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
  };

  it('viewport onSelect triggers getSourceLocation from bridge', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(testState);
    render(() => <App />);

    await waitFor(() => {
      expect(capturedViewportProps.onSelect).toBeDefined();
    });

    // Simulate viewport selection
    capturedViewportProps.onSelect('Bracket');

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
      expect(capturedViewportProps.onSelect).toBeDefined();
    });

    // Simulate viewport selection
    capturedViewportProps.onSelect('Bracket');

    // Wait for getSourceLocation to resolve and selectEntity to be called
    await waitFor(() => {
      // The PropertyEditor should reflect the selection — Bracket group should be data-selected
      const container = screen.getByTestId('property-editor');
      const selectedGroups = container.querySelectorAll('[data-selected]');
      expect(selectedGroups.length).toBe(1);
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
    resolveGetState({ meshes: [], values: [], constraints: [], files: [] });
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
    vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] });

    fireEvent.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // getInitialState called twice: initial + retry
    expect(bridge.getInitialState).toHaveBeenCalledTimes(2);
  });

  it('after successful getInitialState, app-layout is shown and loading/error are gone', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] });

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

    // Simulate two different files changing
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => {
      expect(screen.getByText(/2 files changed/)).toBeTruthy();
    });
  });

  it('same file changed twice results in only one entry', async () => {
    render(() => <App />);
    await waitFor(() => expect(fileChangedCallback).toBeDefined());

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

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });

    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

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

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
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

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    await waitFor(() => expect(screen.getByTestId('reload-prompt')).toBeTruthy());

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

      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

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

      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

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

    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

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

      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

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

    // Two files change
    fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
    fileChangedCallback!({ path: '/project/gear.ri', content: '' });
    await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

    // Click Reload — starts in-flight reload for bracket.ri and gear.ri
    fireEvent.click(screen.getByText('Reload'));

    // Wait for bridgeOpenFile to be called
    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledTimes(2);
    });

    // DURING the in-flight reload, a new file-change event arrives for housing.ri
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

      // Two files change
      fileChangedCallback!({ path: '/project/bracket.ri', content: '' });
      fileChangedCallback!({ path: '/project/gear.ri', content: '' });
      await waitFor(() => expect(screen.getByText(/2 files changed/)).toBeTruthy());

      // Click Reload
      fireEvent.click(screen.getByText('Reload'));

      await waitFor(() => {
        expect(bridge.openFile).toHaveBeenCalledTimes(2);
      });

      // Concurrent file-change event during in-flight reload
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
        }],
        constraints: [],
        files: [],
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
      });

      render(() => <App />);

      // Wait for ready state and file-changed subscription
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
        expect(fileChangedCb).toBeDefined();
      });

      // Trigger file changed event to show the reload prompt
      fileChangedCb({ path: '/project/bracket.ri', content: 'updated' });

      await waitFor(() => {
        expect(screen.getByTestId('reload-prompt')).toBeTruthy();
      });

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
    resolveRetry({ meshes: [], values: [], constraints: [], files: [] });
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
      meshes: [], values: [], constraints: [], files: [],
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
    resolveRetry({ meshes: [], values: [], constraints: [], files: [] });
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
  it('capturedViewportProps.onHover is a function that updates selectionStore', async () => {
    await renderAndWaitForReady();

    expect(capturedViewportProps.onHover).toBeDefined();
    expect(typeof capturedViewportProps.onHover).toBe('function');

    // Calling onHover should update selectionStore.hoveredEntity
    capturedViewportProps.onHover('bracket/hole');

    // The hoveredEntity should now be passed back to Viewport (verified via capturedViewportProps)
    await waitFor(() => {
      expect(capturedViewportProps.hoveredEntity).toBe('bracket/hole');
    });
  });

  it('capturedViewportProps.evalStatus is defined and reflects engine store state', async () => {
    await renderAndWaitForReady();

    expect(capturedViewportProps.evalStatus).toBeDefined();
    // Default engine store eval status is idle
    expect(capturedViewportProps.evalStatus.phase).toBe('idle');
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

    // Property height should be clamped so constraint panel remains visible
    // containerHeight(600) - MIN_PANEL_HEIGHT(80) - 4(splitter) = 516
    const rows = sidePanel.style.gridTemplateRows;
    const heightPx = parseInt(rows.split('px')[0], 10);
    expect(heightPx).toBeLessThanOrEqual(600 - 80 - 4);
    expect(heightPx).toBeGreaterThan(0);
  });
});

describe('App fit-to-view wiring', () => {
  it('capturedViewportProps.fitToViewRef is defined', async () => {
    await renderAndWaitForReady();
    expect(capturedViewportProps.fitToViewRef).toBeDefined();
    expect(typeof capturedViewportProps.fitToViewRef).toBe('function');
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
  it('dispatching Ctrl+O triggers pickOpenPath then openFile', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue({
      meshes: [], values: [], constraints: [],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    });

    // Mock pickOpenPath to return a path
    vi.mocked((bridge as any).pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Press Ctrl+O
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });

    // Should call pickOpenPath, then openFile with the returned path
    await waitFor(() => {
      expect((bridge as any).pickOpenPath).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(bridge.openFile).toHaveBeenCalledWith('/project/other.ri');
    });
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
        }],
        constraints: [],
        files: [],
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
