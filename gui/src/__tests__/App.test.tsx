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
  pickSavePath: vi.fn().mockResolvedValue('/user/chosen/export.step'),
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
}));

import App from '../App';
import * as bridge from '../bridge';

beforeEach(() => {
  vi.clearAllMocks();
  capturedViewportProps = {};
  // Reset getInitialState to default empty state
  vi.mocked(bridge.getInitialState).mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] });
});

afterEach(() => {
  cleanup();
});

describe('App layout wiring', () => {
  it('renders app-layout container', () => {
    render(() => <App />);
    expect(screen.getByTestId('app-layout')).toBeTruthy();
  });

  it('renders Toolbar at top', () => {
    render(() => <App />);
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders StatusBar at bottom', () => {
    render(() => <App />);
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('renders Viewport', () => {
    render(() => <App />);
    expect(screen.getByTestId('viewport-container')).toBeTruthy();
  });

  it('renders Editor', () => {
    render(() => <App />);
    expect(screen.getByTestId('editor-container')).toBeTruthy();
  });

  it('renders PropertyEditor', () => {
    render(() => <App />);
    expect(screen.getByTestId('property-editor')).toBeTruthy();
  });

  it('renders ConstraintPanel', () => {
    render(() => <App />);
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
  });

  it('renders Toolbar before StatusBar in DOM order', () => {
    render(() => <App />);
    const toolbar = screen.getByTestId('toolbar');
    const statusBar = screen.getByTestId('status-bar');
    // Toolbar should come before StatusBar in document order
    const comparison = toolbar.compareDocumentPosition(statusBar);
    expect(comparison & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('App resizable splitters', () => {
  it('has a vertical splitter between editor and viewport columns', () => {
    render(() => <App />);
    const splitter = screen.getByTestId('splitter-left');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('has a vertical splitter between viewport and side panel columns', () => {
    render(() => <App />);
    const splitter = screen.getByTestId('splitter-right');
    expect(splitter).toBeTruthy();
    expect(splitter.dataset.orientation).toBe('vertical');
  });

  it('dragging left splitter updates main grid columns', () => {
    render(() => <App />);
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
  it('has a horizontal splitter between PropertyEditor and ConstraintPanel in the side panel', () => {
    render(() => <App />);
    const sidePanel = screen.getByTestId('side-panel');
    const splitter = sidePanel.querySelector('[data-testid="splitter-side"]');
    expect(splitter).toBeTruthy();
    expect((splitter as HTMLElement).dataset.orientation).toBe('horizontal');
  });

  it('PropertyEditor appears before splitter which appears before ConstraintPanel', () => {
    render(() => <App />);
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

  it('dragging the side panel splitter changes the top/bottom split', () => {
    render(() => <App />);
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
  it('renders FileBrowser in the editor panel', () => {
    render(() => <App />);
    expect(screen.getByTestId('file-browser')).toBeTruthy();
  });

  it('clicking Export in Toolbar opens ExportDialog', async () => {
    render(() => <App />);

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
    render(() => <App />);

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
    render(() => <App />);
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

describe('App handleSetParameter error handling', () => {
  it('logs error when bridge.setParameter rejects', async () => {
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

      // Flush microtask queue for the rejected promise
      await new Promise((r) => setTimeout(r, 0));

      // After fix: console.error is called with 'setParameter failed:' and the error
      // With current code: rejected promise is unhandled, console.error NOT called
      expect(errorSpy).toHaveBeenCalledWith(
        'setParameter failed:',
        expect.any(Error),
      );
    } finally {
      window.removeEventListener('unhandledrejection', rejectHandler);
      errorSpy.mockRestore();
    }
  });
});

describe('App file picker integration (E-6)', () => {
  it('calls pickSavePath then exportGeometry with the chosen path', async () => {
    vi.mocked(bridge.pickSavePath).mockResolvedValue('/user/chosen/export.step');
    vi.mocked(bridge.exportGeometry).mockResolvedValue(undefined);

    render(() => <App />);

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog (default format is 'step')
    const dialog = screen.getByRole('dialog');
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

    render(() => <App />);

    // Open the export dialog
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => {
      expect(screen.getByTestId('export-dialog')).toBeTruthy();
    });

    // Click Export inside the dialog
    const dialog = screen.getByRole('dialog');
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
