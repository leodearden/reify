import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@solidjs/testing-library';
import type { GuiState } from '../types';

// Mock Tauri APIs before any component imports
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

// Capture Viewport props for navigation tests
let capturedViewportProps: any = {};
vi.mock('../viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportProps = props;
    // Invoke flyToEntityRef with a mock function if provided
    if (props.flyToEntityRef) {
      props.flyToEntityRef((_path: string) => {});
    }
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'viewport-container');
    el.textContent = 'Viewport Mock';
    return el;
  },
}));

// Mock Editor (requires CodeMirror DOM APIs)
vi.mock('../editor/Editor', () => ({
  Editor: (_props: any) => {
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
  updateSource: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockResolvedValue({ path: '', content: '' }),
  getSourceLocation: vi.fn().mockResolvedValue({ file: '/test.ri', line: 1, column: 1, end_line: 1, end_column: 5 }),
  focusEntity: vi.fn().mockResolvedValue(undefined),
  onMeshUpdate: vi.fn().mockResolvedValue(() => {}),
  onValueUpdate: vi.fn().mockResolvedValue(() => {}),
  onConstraintUpdate: vi.fn().mockResolvedValue(() => {}),
  onEvaluationStatus: vi.fn().mockResolvedValue(() => {}),
  onMeshRemoved: vi.fn().mockResolvedValue(() => {}),
  onValueRemoved: vi.fn().mockResolvedValue(() => {}),
  onConstraintRemoved: vi.fn().mockResolvedValue(() => {}),
  onFileChanged: vi.fn().mockResolvedValue(() => {}),
  pickSavePath: vi.fn().mockResolvedValue('/user/chosen/path.step'),
}));

import App from '../App';
import * as bridge from '../bridge';

beforeEach(() => {
  vi.clearAllMocks();
  capturedViewportProps = {};
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
    let resolveMeshListen!: (unsub: () => void) => void;
    vi.mocked(bridge.onMeshUpdate).mockReturnValue(
      new Promise<() => void>((resolve) => { resolveMeshListen = resolve; }),
    );

    // All other event listeners resolve immediately with tracked unlistens
    vi.mocked(bridge.onValueUpdate).mockResolvedValue(valueUnlisten);
    vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(constraintUnlisten);
    vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(evalUnlisten);
    vi.mocked(bridge.onMeshRemoved).mockResolvedValue(meshRemovedUnlisten);
    vi.mocked(bridge.onValueRemoved).mockResolvedValue(valueRemovedUnlisten);
    vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(constraintRemovedUnlisten);

    const { unmount } = render(() => <App />);

    // Wait for getInitialState to resolve and subscribeToEvents to start
    await new Promise((r) => setTimeout(r, 0));

    // Unmount while subscribeToEvents is still pending (waiting for deferred onMeshUpdate)
    unmount();

    // Resolve the deferred onMeshUpdate — subscribeToEvents will now complete
    resolveMeshListen(meshUnlisten);

    // Flush microtasks so subscribeToEvents' await resolves
    await new Promise((r) => setTimeout(r, 0));

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
    let resolveGetState!: (state: GuiState) => void;
    vi.mocked(bridge.getInitialState).mockReturnValue(
      new Promise<GuiState>((resolve) => { resolveGetState = resolve; }),
    );

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
      }],
      constraints: [],
      files: [{ path: '/test.ri', content: '' }],
    });

    // Flush microtasks
    await new Promise((r) => setTimeout(r, 0));

    // After fix: alive guard returns before reaching subscribeToEvents
    // With current code: initFromState runs, then subscribeToEvents runs → onMeshUpdate called
    expect(bridge.onMeshUpdate).not.toHaveBeenCalled();
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
      },
    ],
    constraints: [
      {
        node_id: 'n1',
        expression: 'width > 0',
        status: 'violated',
        details: null,
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
    let resolveGetState!: (state: GuiState) => void;
    vi.mocked(bridge.getInitialState).mockReturnValue(
      new Promise<GuiState>((resolve) => { resolveGetState = resolve; }),
    );

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

describe('App handleSetParameter error handling', () => {
  it('shows error toast when bridge.setParameter rejects', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Prevent unhandled rejection from failing the test
    const rejectHandler = (e: any) => e.preventDefault();
    window.addEventListener('unhandledrejection', rejectHandler);

    try {
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
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'setParameter failed:',
        expect.any(Error),
      );
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
      errorSpy.mockRestore();
    }
  });
});

describe('App re-evaluate error toast', () => {
  it('shows error toast when re-evaluate (F5) fails', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const rejectHandler = (e: any) => e.preventDefault();
    window.addEventListener('unhandledrejection', rejectHandler);

    try {
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
      });

      // console.error should NOT be called
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Re-evaluate failed:',
        expect.any(Error),
      );
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
      errorSpy.mockRestore();
    }
  });
});

describe('App event subscription error toast', () => {
  it('shows warning toast when subscribeToEvents fails', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const rejectHandler = (e: any) => e.preventDefault();
    window.addEventListener('unhandledrejection', rejectHandler);

    try {
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
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
      errorSpy.mockRestore();
    }
  });
});

describe('App reload error toast', () => {
  it('shows error toast when reload fails', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const rejectHandler = (e: any) => e.preventDefault();
    window.addEventListener('unhandledrejection', rejectHandler);

    try {
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
        expect(toastEl.textContent).toContain('Reload failed');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Reload failed:',
        expect.any(Error),
      );
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
      errorSpy.mockRestore();
    }
  });
});

describe('App export with save dialog', () => {
  it('handleDoExport calls pickSavePath before exportGeometry', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/path.step');
    await renderAndWaitForReady();

    // Open export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export button inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:not([class*="secondary"])') as HTMLElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
      expect(bridge.exportGeometry).toHaveBeenCalledWith('step', '/user/chosen/path.step');
    });
  });

  it('if pickSavePath returns null (user cancelled), exportGeometry is NOT called', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue(null);
    await renderAndWaitForReady();

    // Open export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export button inside dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:not([class*="secondary"])') as HTMLElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(bridge.pickSavePath).toHaveBeenCalled();
    });

    // exportGeometry should NOT be called when user cancels
    expect(bridge.exportGeometry).not.toHaveBeenCalled();
  });

  it('if pickSavePath throws (not available), falls back to default path', async () => {
    vi.mocked(bridge.pickSavePath).mockRejectedValue(new Error('command not found'));
    await renderAndWaitForReady();

    // Open export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export button inside dialog
    const dialog = screen.getByTestId('export-dialog');
    const exportBtn = dialog.querySelector('button:not([class*="secondary"])') as HTMLElement;
    fireEvent.click(exportBtn);

    await waitFor(() => {
      // Should fall back to hardcoded default path
      expect(bridge.exportGeometry).toHaveBeenCalledWith('step', 'export.step');
    });
  });
});

describe('App end-to-end toast integration', () => {
  it('App renders, loads state (ready), then setParameter failure shows toast with correct message', async () => {
    const rejectHandler = (e: any) => e.preventDefault();
    window.addEventListener('unhandledrejection', rejectHandler);

    try {
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
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
    }
  });
});
