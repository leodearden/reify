import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import { withSuppressedRejections } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';
import { splitGridTracks } from './test_utils/gridTracks';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStubModule());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStubModule());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStubModule());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import { STORAGE_KEY } from '../hooks/useLayoutPersistence';
import { VIEWPORT_DISPLAY_STORAGE_KEY } from '../stores/viewportDisplayStore';
import { primeAppHarness, renderAndWaitForReady, oneFileState } from './appHarness/harness';
import {
  capturedDualViewportProps,
  capturedMultiViewportProps,
  mockViewportFitToView,
} from './appHarness/renderStubs';
import { makeMesh } from './test_utils/meshData';

beforeEach(primeAppHarness);

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

describe('edges-in-black toggle wiring', () => {
  const edgesButton = () => screen.getByTestId('toolbar-edges-toggle');
  const storedEdges = () => JSON.parse(localStorage.getItem(VIEWPORT_DISPLAY_STORAGE_KEY) ?? 'null');

  it('the E key and the toolbar button both toggle edges, persist them, and reach the viewport', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
    await renderAndWaitForReady();
    expect(edgesButton().getAttribute('aria-pressed')).toBe('false');
    expect(capturedDualViewportProps.showEdges).toBe(false);

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));

    expect(edgesButton().getAttribute('aria-pressed')).toBe('true');
    expect(storedEdges()).toEqual({ edges: true });
    expect(capturedDualViewportProps.showEdges).toBe(true);

    fireEvent.click(edgesButton());

    expect(edgesButton().getAttribute('aria-pressed')).toBe('false');
    expect(storedEdges()).toEqual({ edges: false });
    expect(capturedDualViewportProps.showEdges).toBe(false);
  });

  it('starts pressed when edges were left on in a previous session', async () => {
    localStorage.setItem(VIEWPORT_DISPLAY_STORAGE_KEY, JSON.stringify({ edges: true }));

    await renderAndWaitForReady();

    expect(edgesButton().getAttribute('aria-pressed')).toBe('true');
  });

  it('reaches every pane of a display_panes layout', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: [
        { subject: 'A#realization[0]', pane: 0 },
        { subject: 'B#realization[0]', pane: 1 },
      ],
    })));
    await renderAndWaitForReady();
    const panes = () => capturedMultiViewportProps.panes as Array<{ showEdges?: boolean }>;
    expect(panes()).toHaveLength(2);
    expect(panes().map((p) => p.showEdges)).toEqual([false, false]);

    fireEvent.click(edgesButton());

    expect(panes().map((p) => p.showEdges)).toEqual([true, true]);
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

describe('App end-to-end toast integration', () => {
  it('App renders, loads state (ready), then setParameter failure shows toast with correct message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('backend unavailable'));
      vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
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
      })));

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
  beforeEach(() => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
  });

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

describe('App diagnostics panel splitter (step 11)', () => {
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
    line: 3, column: 1, end_line: 3, end_column: 10,
    severity: 'Warning',
    message: 'splitter test warning',
    code: null,
  };

  it('splitter-problems is absent when panel is collapsed (default)', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());
    compileDiagnosticsCallback!([warningDiag]);
    await waitFor(() => expect(screen.getByTestId('diagnostics-count')).toBeTruthy());

    // Default: panel is collapsed — splitter should not be in the DOM
    expect(document.querySelector('[data-testid="splitter-problems"]')).toBeNull();
  });

  it('splitter-problems is present inside editor-panel when panel is expanded', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());
    compileDiagnosticsCallback!([warningDiag]);
    await waitFor(() => expect(screen.getByTestId('diagnostics-count')).toBeTruthy());

    // Expand the panel
    fireEvent.click(screen.getByTestId('diagnostics-count'));
    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });

    // Splitter should now appear inside the editor panel
    const editorPanel = screen.getByTestId('editor-panel');
    expect(editorPanel.querySelector('[data-testid="splitter-problems"]')).toBeTruthy();
  });

  it('dragging splitter-problems downward decreases panel height (handleProblemsResize wiring)', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());
    compileDiagnosticsCallback!([warningDiag]);
    await waitFor(() => expect(screen.getByTestId('diagnostics-count')).toBeTruthy());

    // Expand the panel
    fireEvent.click(screen.getByTestId('diagnostics-count'));
    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });

    // Mock editor-panel height so clampProblemsHeight has a known upper bound
    const editorPanel = screen.getByTestId('editor-panel');
    Object.defineProperty(editorPanel, 'clientHeight', { value: 600, configurable: true });

    const splitter = editorPanel.querySelector('[data-testid="splitter-problems"]') as HTMLElement;
    expect(splitter).toBeTruthy();

    // Drag DOWN by 20px — panel is BELOW the splitter so h - delta shrinks it:
    // clampProblemsHeight(160 - 20, 600, {min:80, editorMin:80, splitterT:4}) = 140
    fireEvent.mouseDown(splitter, { clientX: 0, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 0, clientY: 220 });
    fireEvent.mouseUp(document);

    await waitFor(() => {
      const panel = screen.getByTestId('diagnostics-panel');
      expect(panel.style.height).toBe('140px');
    });
  });

  it('oversized persisted problemsHeight is clamped when editor-panel ResizeObserver fires', async () => {
    // Seed an oversized problemsHeight with panel expanded so the height is visible in the DOM.
    // editorWidth + sideWidth are required by loadPanelLayout's validation check; without them
    // loadPanelLayout returns null and defaults are used (problemsCollapsed: true, height: 160).
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      editorWidth: 300,
      sideWidth: 300,
      problemsHeight: 500,
      problemsCollapsed: false,
    }));

    // Capture all ResizeObserver callbacks in insertion order:
    // [0] = editorPanel observer (createEffect at ~line 1164)
    // [1] = sidePanel observer  (createEffect at ~line 1178)
    const roCallbacks: ResizeObserverCallback[] = [];
    const OrigRO = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
      constructor(cb: ResizeObserverCallback) { roCallbacks.push(cb); }
    } as any;

    try {
      await renderAndWaitForReady();

      // First observer is for the editor panel
      expect(roCallbacks.length).toBeGreaterThanOrEqual(1);
      const editorPanel = screen.getByTestId('editor-panel');
      Object.defineProperty(editorPanel, 'clientHeight', { value: 300, configurable: true });

      // Fire the editor-panel ResizeObserver callback (entry payload unused by handler)
      roCallbacks[0]([{ contentRect: { width: 500, height: 300 } }] as any, {} as any);

      // clampProblemsHeight(500, 300, {min:80, editorMin:80, splitterT:4}):
      //   available = 300 - 80 - 4 = 216
      //   result    = max(80, min(500, 216)) = 216
      await waitFor(() => {
        const panel = screen.getByTestId('diagnostics-panel');
        expect(panel.style.height).toBe('216px');
      });
    } finally {
      globalThis.ResizeObserver = OrigRO;
    }
  });
});

// ── WarmPoolDebugPanel placement (task 4279) ─────────────────────────────────

describe('App warm-pool debug panel placement (task 4279)', () => {
  it('DEBUG ON: panel renders outside side-panel grid, in floating overlay', async () => {
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    await renderAndWaitForReady();

    // Panel must be visible behind the debug affordance
    await waitFor(() => expect(screen.getByTestId('warm-pool-debug-panel')).toBeTruthy());

    // Overlay wrapper must exist
    expect(screen.getByTestId('warm-pool-debug-overlay')).toBeTruthy();

    // Panel must be inside the overlay wrapper
    expect(screen.getByTestId('warm-pool-debug-overlay').contains(screen.getByTestId('warm-pool-debug-panel'))).toBe(true);

    // Panel must NOT be a descendant of the side-panel grid
    const sidePanel = screen.getByTestId('side-panel');
    expect(sidePanel.contains(screen.getByTestId('warm-pool-debug-panel'))).toBe(false);

    // Side-panel grid invariant: tracked children == template tracks (no 8px strip)
    expect(splitGridTracks(sidePanel.style.gridTemplateRows).length).toBe(sidePanel.children.length);
  });

  it('DEBUG OFF: neither panel nor overlay renders in the layout', async () => {
    await renderAndWaitForReady();

    expect(screen.queryByTestId('warm-pool-debug-panel')).toBeNull();
    expect(screen.queryByTestId('warm-pool-debug-overlay')).toBeNull();
  });
});
