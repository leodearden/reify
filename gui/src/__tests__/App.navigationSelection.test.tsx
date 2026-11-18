import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import type { GuiState } from '../types';
import type { ReferenceResult } from '../editor/references';
import { flushMacrotasks, deferred } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStub());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStub());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStub());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import { primeAppHarness, renderAndWaitForReady, oneFileState } from './appHarness/harness';
import {
  capturedDualViewportProps,
  mockFlyToEntity,
  capturedEditorStore,
  capturedEditorScrollToLocation,
  capturedEditorOnShowReferences,
} from './appHarness/renderStubs';

beforeEach(primeAppHarness);

describe('App navigation wiring', () => {
  const testState = makeGuiState({
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
  });

  it('viewport onSelect triggers getSourceLocation from bridge', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));

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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
    // testState opens no files, so the reveal must open the named one first.
    vi.mocked(bridge.openFile).mockResolvedValueOnce({ path: '/project/bracket.ri', content: 'structure Bracket {}' });

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
    expect(bridge.openFile).toHaveBeenCalledWith('/project/bracket.ri');
    expect(capturedEditorStore.state.activeFile).toBe('/project/bracket.ri');
  });

  function twoFileState(active: string, other: string): GuiState {
    return makeGuiState({
      files: [
        { path: active, content: '' },
        { path: other, content: '' },
      ],
    });
  }

  /** Renders App with both files open and `active` the active tab. */
  async function renderWithTabs(active: string, other: string) {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(twoFileState(active, other)));
    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore?.state.openFiles).toHaveLength(2));
    capturedEditorStore.setActiveFile(active);
  }

  it('navigate-to-source for an open, non-active tab activates it without opening it again', async () => {
    let capturedNavigateCb: ((data: { file: string; line: number; column: number; end_line: number; end_column: number }) => void) | undefined;
    vi.mocked(bridge.onNavigateToSource).mockImplementation(async (cb) => {
      capturedNavigateCb = cb;
      return () => {};
    });
    await renderWithTabs('/project/bracket.ri', '/project/gear.ri');

    capturedNavigateCb!({ file: '/project/gear.ri', line: 3, column: 2, end_line: 5, end_column: 7 });

    await waitFor(() => expect(capturedEditorStore.state.activeFile).toBe('/project/gear.ri'));
    await waitFor(() =>
      expect(capturedEditorScrollToLocation?.()).toEqual({
        file_path: '/project/gear.ri', line: 3, column: 2, end_line: 5, end_column: 7,
      }),
    );
    expect(bridge.openFile).not.toHaveBeenCalled();
  });

  it('viewport click whose source is an open, non-active tab activates it, scrolls, and selects', async () => {
    const span = { file_path: '/proj/b.ri', line: 4, column: 1, end_line: 9, end_column: 2 };
    vi.mocked(bridge.getSourceLocation).mockResolvedValue(span);
    await renderWithTabs('/proj/a.ri', '/proj/b.ri');

    capturedDualViewportProps.onSelect('Bracket');

    await waitFor(() => expect(capturedEditorStore.state.activeFile).toBe('/proj/b.ri'));
    await waitFor(() => expect(capturedEditorScrollToLocation?.()).toEqual(span));
    expect(capturedDualViewportProps.selectedEntity).toBe('Bracket');
  });

  it('viewport click whose source cannot be opened still selects, warns, and raises no toast', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      vi.mocked(bridge.getSourceLocation).mockResolvedValue({
        file_path: '/proj/missing.ri', line: 1, column: 1, end_line: 1, end_column: 5,
      });
      vi.mocked(bridge.openFile).mockRejectedValueOnce(new Error('No such file or directory'));
      await renderWithTabs('/proj/a.ri', '/proj/b.ri');

      capturedDualViewportProps.onSelect('Bracket');

      await waitFor(() =>
        expect(warn.mock.calls.some((args) => args.join(' ').includes('missing.ri'))).toBe(true),
      );
      expect(capturedDualViewportProps.selectedEntity).toBe('Bracket');
      expect(
        screen.queryAllByTestId('toast').filter((t) => t.textContent?.includes('missing.ri')),
      ).toHaveLength(0);
      expect(capturedEditorStore.state.activeFile).toBe('/proj/a.ri');
    } finally {
      warn.mockRestore();
    }
  });

  it('a viewport reveal still opening its file does not override a newer navigate-to-source', async () => {
    let capturedNavigateCb: ((data: { file: string; line: number; column: number; end_line: number; end_column: number }) => void) | undefined;
    vi.mocked(bridge.onNavigateToSource).mockImplementation(async (cb) => {
      capturedNavigateCb = cb;
      return () => {};
    });
    vi.mocked(bridge.getSourceLocation).mockResolvedValue({
      file_path: '/proj/slow.ri', line: 1, column: 1, end_line: 1, end_column: 5,
    });
    const slowOpen = deferred<{ path: string; content: string }>();
    vi.mocked(bridge.openFile).mockReturnValueOnce(slowOpen.promise);
    await renderWithTabs('/proj/a.ri', '/proj/b.ri');
    const bSpan = { file_path: '/proj/b.ri', line: 3, column: 2, end_line: 5, end_column: 7 };

    capturedDualViewportProps.onSelect('Bracket');
    await waitFor(() => expect(bridge.openFile).toHaveBeenCalledWith('/proj/slow.ri'));
    capturedNavigateCb!({ file: '/proj/b.ri', line: 3, column: 2, end_line: 5, end_column: 7 });
    await waitFor(() => expect(capturedEditorScrollToLocation?.()).toEqual(bSpan));
    slowOpen.resolve({ path: '/proj/slow.ri', content: '' });
    await flushMacrotasks();

    expect(capturedEditorStore.state.activeFile).toBe('/proj/b.ri');
    expect(capturedEditorStore.state.openFiles.some((f: { path: string }) => f.path === '/proj/slow.ri')).toBe(false);
    expect(capturedEditorScrollToLocation?.()).toEqual(bSpan);
  });
});

describe('Find-uses panel navigation (task 9203)', () => {
  const mainOnlyState = makeGuiState({
    files: [{ path: '/proj/main.ri', content: 'structure Main {}' }],
  });

  function reference(uri: string): ReferenceResult {
    return { uri, line: 2, character: 4, endLine: 2, endCharacter: 9 };
  }

  /** Renders App with main.ri active; the returned function shows `results` and clicks the first row. */
  async function renderWithReferences(results: ReferenceResult[]): Promise<() => Promise<void>> {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(mainOnlyState));
    render(() => <App />);
    await waitFor(() => expect(capturedEditorOnShowReferences).toBeDefined());
    await waitFor(() => expect(capturedEditorStore?.state.activeFile).toBe('/proj/main.ri'));
    return async () => {
      capturedEditorOnShowReferences!(results);
      fireEvent.click(await waitFor(() => screen.getByTestId('find-use-row')));
    };
  }

  it('cross-file, not open: opens the PATH (not the URI), activates it, and scrolls under its key', async () => {
    const clickReference = await renderWithReferences([reference('file:///proj/parts.ri')]);
    vi.mocked(bridge.openFile).mockResolvedValueOnce({ path: '/proj/parts.ri', content: 'structure Part {}' });

    await clickReference();

    await waitFor(() => expect(bridge.openFile).toHaveBeenCalledWith('/proj/parts.ri'));
    await waitFor(() => expect(capturedEditorStore.state.activeFile).toBe('/proj/parts.ri'));
    await waitFor(() =>
      expect(capturedEditorScrollToLocation?.()).toEqual({
        file_path: '/proj/parts.ri', line: 3, column: 5, end_line: 3, end_column: 10,
      }),
    );
    await waitFor(() => expect(screen.queryByTestId('find-uses-panel')).toBeNull());
  });

  it('cross-file, already open: activates the existing tab without opening it again', async () => {
    const clickReference = await renderWithReferences([reference('file:///proj/parts.ri')]);
    capturedEditorStore.openFile({ path: '/proj/parts.ri', content: 'structure Part {}' });
    capturedEditorStore.setActiveFile('/proj/main.ri');

    await clickReference();

    await waitFor(() => expect(capturedEditorStore.state.activeFile).toBe('/proj/parts.ri'));
    await waitFor(() =>
      expect(capturedEditorScrollToLocation?.()).toEqual({
        file_path: '/proj/parts.ri', line: 3, column: 5, end_line: 3, end_column: 10,
      }),
    );
    expect(bridge.openFile).not.toHaveBeenCalled();
  });

  it('same-file: scrolls under the path spelling without opening anything', async () => {
    const clickReference = await renderWithReferences([reference('file:///proj/main.ri')]);

    await clickReference();

    await waitFor(() =>
      expect(capturedEditorScrollToLocation?.()).toEqual({
        file_path: '/proj/main.ri', line: 3, column: 5, end_line: 3, end_column: 10,
      }),
    );
    expect(bridge.openFile).not.toHaveBeenCalled();
    expect(capturedEditorStore.state.activeFile).toBe('/proj/main.ri');
  });
});

describe('App Escape clears multi-selection', () => {
  beforeEach(() => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
  });

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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
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
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(oneFileState()));
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

// ---------------------------------------------------------------------------
// App hover sync wiring — Edge C (task-4209)
// Edge C: editor cursor → createEditorHoverSync → store → DualViewport.hoveredEntity
// ---------------------------------------------------------------------------

describe('App hover sync wiring — Edge C', () => {
  it('(a) editor cursor change routes hover through createEditorHoverSync to DualViewport.hoveredEntity', async () => {
    // Bridge returns 'Bracket.width' for any source location lookup
    vi.mocked((bridge as any).getEntityAtSourceLocation).mockResolvedValue('Bracket.width');

    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    vi.useFakeTimers();
    try {
      capturedEditorStore.setCursorPosition(2, 11);
      await vi.advanceTimersByTimeAsync(250);
      // Flush microtasks so Promise resolutions propagate
      await Promise.resolve();
      await Promise.resolve();

      // createEditorHoverSync must have resolved the position and called hoverEntity;
      // do NOT constrain getEntityAtSourceLocation call count — both hover and selection
      // sync hooks call it for the same cursor position.
      await waitFor(() => {
        expect(capturedDualViewportProps.hoveredEntity).toBe('Bracket.width');
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('(b) null cursor position clears DualViewport.hoveredEntity', async () => {
    vi.mocked((bridge as any).getEntityAtSourceLocation).mockResolvedValue('Bracket.width');

    await renderAndWaitForReady();
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    vi.useFakeTimers();
    try {
      // First set a cursor to establish hover
      capturedEditorStore.setCursorPosition(2, 11);
      await vi.advanceTimersByTimeAsync(250);
      await Promise.resolve();
      await Promise.resolve();
      await waitFor(() => expect(capturedDualViewportProps.hoveredEntity).toBe('Bracket.width'));

      // Then clear cursor — hook must immediately call hoverEntity(null)
      capturedEditorStore.setCursorPosition(null);
      await vi.advanceTimersByTimeAsync(0);
      await Promise.resolve();
      await waitFor(() => expect(capturedDualViewportProps.hoveredEntity).toBeNull());
    } finally {
      vi.useRealTimers();
    }
  });
});

// ---------------------------------------------------------------------------
// App hover sync wiring — Edges A & B (task-4209)
// Edge A: DesignTree row mouseenter → store → DualViewport.hoveredEntity
// Edge B: DualViewport.onHover → store → DesignTree row data-hovered
// ---------------------------------------------------------------------------

describe('App hover sync wiring — Edges A & B', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  it('(a) Edge A: DesignTree row mouseenter routes hover through store to DualViewport.hoveredEntity', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('tree-row-Root.A')).toBeTruthy());

    fireEvent.mouseEnter(screen.getByTestId('tree-row-Root.A'));

    // Outline hover → selectionStore.hoverEntity → DualViewport receives hoveredEntity prop
    await waitFor(() => expect(capturedDualViewportProps.hoveredEntity).toBe('Root.A'));
  });

  it('(a-clear) Edge A mouseleave clears DualViewport.hoveredEntity', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('tree-row-Root.A')).toBeTruthy());

    // Hover on, then off
    fireEvent.mouseEnter(screen.getByTestId('tree-row-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.hoveredEntity).toBe('Root.A'));
    fireEvent.mouseLeave(screen.getByTestId('tree-row-Root.A'));
    await waitFor(() => expect(capturedDualViewportProps.hoveredEntity).toBeNull());
  });

  it('(b) Edge B: DualViewport.onHover routes through store to DesignTree row data-hovered attribute', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A'), makeNode('Root.B')]);
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('tree-row-Root.A')).toBeTruthy());

    // Simulate viewport hover arriving via the existing onHover prop
    capturedDualViewportProps.onHover('Root.A');

    // The matching row should receive data-hovered='true'; the other should not
    await waitFor(() => expect(screen.getByTestId('tree-row-Root.A').getAttribute('data-hovered')).toBe('true'));
    expect(screen.getByTestId('tree-row-Root.B').getAttribute('data-hovered')).toBeNull();
  });
});

// ── App reconcileToTree wiring (step-7) ─────────────────────────────────────
describe('App refreshEntityTree wires reconcileToTree (step-7)', () => {
  it('orphan mesh absent from getEntityTree tree is pruned after refreshEntityTree', async () => {
    // Live mesh owner is in the tree; orphan mesh owner is NOT
    const liveMeshKey = 'CapstanDrive.shuttle.plate#realization0';
    const orphanMeshKey = 'ShuttlePlates#realization0';

    const initialState = makeGuiState({
      meshes: [
        { entity_path: liveMeshKey, vertices: new Float32Array([0, 1, 2]), indices: new Uint32Array([0, 1, 2]), normals: null },
        { entity_path: orphanMeshKey, vertices: new Float32Array([3, 4, 5]), indices: new Uint32Array([0, 1, 2]), normals: null },
      ],
    });

    // Seed both meshes via getInitialState
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(initialState));
    // Only the live structure is in the tree
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      { entity_path: 'CapstanDrive.shuttle.plate', kind: 'structure', type_name: null, has_mesh: true, trait_geometry: false, freshness: 'final', children: [] },
    ]);

    await renderAndWaitForReady();
    // Wait for the entity tree to be fetched (triggers reconcileToTree if wired up)
    await waitFor(() => expect(bridge.getEntityTree).toHaveBeenCalledTimes(1));

    // DualViewport receives engineStore as a prop; read meshes through it
    const meshesRef = () => capturedDualViewportProps?.engineStore?.state?.meshes ?? {};

    // After refreshEntityTree resolves, reconcileToTree should have pruned the orphan
    await waitFor(() => {
      const meshKeys = Object.keys(meshesRef());
      expect(meshKeys).not.toContain(orphanMeshKey);
      expect(meshKeys).toContain(liveMeshKey);
    });
  });
});

// ── App epoch/staleness guard (task 4251 step-3/step-4) ─────────────────────
//
// Scenario: an in-flight bridgeGetEntityTree() fetch that was issued BEFORE an
// engine reinit (i.e. before a new design was loaded) must be DROPPED when it
// resolves, so its (partially-stale) reconcileToTree call does not prune meshes
// that belong to the freshly-loaded design.
//
// The stale snapshot is deliberately crafted to be a PARTIAL overlap with the
// current design (it contains CapstanDrive.capstan but NOT
// CapstanDrive.shuttle.plate). This means the step-2 cross-root guard does
// NOT engage — the test isolates the epoch/staleness guard implemented in step-4.
describe('App epoch/staleness guard for refreshEntityTree (task 4251)', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return {
      entity_path,
      kind: 'structure' as const,
      type_name: null,
      has_mesh: true,
      trait_geometry: false,
      freshness: 'final' as const,
      children,
    };
  }

  it('stale in-flight entity-tree snapshot fetched before an engine reinit does not prune the freshly-loaded design', async () => {
    // Design A: loaded by getInitialState during initApp
    const designAState = makeGuiState({
      meshes: [
        {
          entity_path: 'PrinterFrame.gantry#r0',
          vertices: new Float32Array([0, 1, 2]),
          indices: new Uint32Array([0, 1, 2]),
          normals: null,
        },
      ],
    });

    // Design B: loaded directly via initFromState (simulates a file-switch reinit)
    const designBState = makeGuiState({
      meshes: [
        {
          entity_path: 'CapstanDrive.capstan#r0',
          vertices: new Float32Array([0, 1, 2]),
          indices: new Uint32Array([0, 1, 2]),
          normals: null,
        },
        {
          entity_path: 'CapstanDrive.shuttle.plate#r0',
          vertices: new Float32Array([3, 4, 5]),
          indices: new Uint32Array([0, 1, 2]),
          normals: null,
        },
      ],
    });

    // Deferred promise for the FIRST getEntityTree call (the stale in-flight fetch).
    // It is issued during initApp → initFromState → onEngineReinitialized → refreshEntityTree.
    const staleDeferred = deferred<any[]>();

    vi.mocked(bridge.getInitialState).mockResolvedValue(published(designAState));
    // First call: stale, stays in-flight for the duration of the test
    vi.mocked(bridge.getEntityTree).mockReturnValueOnce(staleDeferred.promise);
    // Subsequent calls (from the reinit's refreshEntityTree): return design B's tree immediately
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      makeNode('CapstanDrive.capstan'),
      makeNode('CapstanDrive.shuttle.plate'),
    ]);

    await renderAndWaitForReady();

    // Capture the engine store exposed through DualViewport props
    const engine = capturedDualViewportProps.engineStore;
    expect(engine).toBeDefined();

    // Trigger a reinit with design B — fires onEngineReinitialized → epoch bump
    // (step-4 will add this) + fresh refreshEntityTree against design B's tree.
    engine.initFromState(designBState);

    // Wait for the fresh post-reinit fetch to resolve and reconcile design B's meshes.
    // Both B meshes are present because the fresh tree lists both.
    await waitFor(() => {
      const meshKeys = Object.keys(engine.state.meshes);
      expect(meshKeys).toContain('CapstanDrive.capstan#r0');
      expect(meshKeys).toContain('CapstanDrive.shuttle.plate#r0');
    });

    // Now resolve the stale in-flight fetch with a PARTIAL-overlap tree.
    // It includes CapstanDrive.capstan (shared with design B) so the step-2
    // cross-root guard does NOT engage. The only guard that should stop pruning
    // is the epoch/staleness guard implemented in step-4.
    staleDeferred.resolve([makeNode('CapstanDrive.capstan')]);

    // Flush the macrotask queue so the stale .then callback (and any further
    // microtask hops it spawns) is guaranteed to have run before we assert.
    // Using flushMacrotasks() rather than bare await Promise.resolve() ticks
    // is more durable: a setTimeout barrier drains all pending microtasks
    // first, so if the resolve path ever gains an extra microtask hop the
    // assertion ordering remains correct.
    await flushMacrotasks();

    // Epoch guard (step-4) must have dropped the stale snapshot.
    // Both B meshes must survive — shuttle.plate must NOT have been pruned.
    expect(Object.keys(engine.state.meshes)).toContain('CapstanDrive.capstan#r0');
    expect(Object.keys(engine.state.meshes)).toContain('CapstanDrive.shuttle.plate#r0');
    // The viewport must stay active (not collapse to 'No active viewport')
    expect(capturedDualViewportProps.designViewportActive()).toBe(true);
  });
});
