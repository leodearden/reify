import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import { flushMacrotasks } from './test-utils';

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
import * as sidecarPersistence from '../stores/sidecarPersistence';
import * as viewPersistence from '../stores/viewPersistence';
import { SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS } from '../stores/engineStore';
import { renderAndWaitForReady } from './appHarness/harness';
import { capturedDualViewportProps } from './appHarness/renderStubs';

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
// Step-9 (task-4768 ε): Layout state restoration tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — layout state restoration (task-4768 ε)', () => {
  async function openFileWithLayout(
    path: string,
    viewportLayout: Record<string, { sizeWeight: number; forceExpanded: boolean }>,
    splitRatio: number,
  ) {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(path);
    vi.mocked(bridge.openFile).mockResolvedValue({ path, content: '' });
    vi.mocked(sidecarPersistence.loadSidecar).mockResolvedValue({
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: {},
      viewportLayout,
      splitRatio,
      timestamp: '2026-04-23T00:00:00.000Z',
    });

    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    for (let i = 0; i < 10; i++) await Promise.resolve();
  }

  it('restores sizeWeight, forceExpanded, and splitRatio from sidecar on openFile', async () => {
    await renderAndWaitForReady();
    await openFileWithLayout('/test/bracket.ri', {
      'design-main': { sizeWeight: 3, forceExpanded: true },
      'def-preview': { sizeWeight: 2, forceExpanded: false },
    }, 0.25);

    await waitFor(() => {
      const store = capturedDualViewportProps.viewportStore;
      expect(store?.state.viewports['design-main'].sizeWeight).toBe(3);
      expect(store?.state.viewports['design-main'].forceExpanded).toBe(true);
      expect(store?.state.viewports['def-preview'].sizeWeight).toBe(2);
      expect(store?.state.splitRatio).toBeCloseTo(0.25);
    });
  });
});

// ---------------------------------------------------------------------------
// Step-11 (task-4768 ε): Layout state compose/save tests
// ---------------------------------------------------------------------------

describe('App persistence wiring — layout state compose (task-4768 ε)', () => {
  function makeNode(entity_path: string, children: any[] = []) {
    return { entity_path, kind: 'structure', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children };
  }

  async function openFile(path = '/test/bracket.ri') {
    vi.mocked(bridge.pickOpenPath).mockResolvedValue(path);
    vi.mocked(bridge.openFile).mockResolvedValue({ path, content: '' });
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await waitFor(() => expect(sidecarPersistence.loadSidecar).toHaveBeenCalledWith(path));
  }

  async function openViewSelectorDropdown() {
    await waitFor(() => expect(screen.getByRole('button', { name: 'Default' })).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Default' }));
  }

  it('sidecar save includes viewportLayout and splitRatio after store mutations', async () => {
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await openFile();

    // Mutate the captured viewportStore
    const store = capturedDualViewportProps.viewportStore;
    store.setSizeWeight('design-main', 5);
    store.setForceExpanded('design-main', true);
    store.setSplitRatio(0.7);

    // Trigger the sidecar save via the "Save views" menu item
    await openViewSelectorDropdown();
    fireEvent.click(screen.getByRole('menuitem', { name: /save views/i }));

    await waitFor(() => {
      expect(sidecarPersistence.saveSidecar).toHaveBeenCalledWith(
        '/test/bracket.ri',
        expect.objectContaining({
          viewportLayout: expect.objectContaining({
            'design-main': { sizeWeight: 5, forceExpanded: true },
          }),
          splitRatio: expect.closeTo(0.7, 5),
        }),
      );
    });
  });

  it('debounced localStorage save also includes viewportLayout and splitRatio', async () => {
    const { saveViewPersistence: realSave } = await import('../stores/viewPersistence');
    const saveSpy = vi.spyOn(await import('../stores/viewPersistence'), 'saveViewPersistence');

    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Root.A')]);
    await renderAndWaitForReady();
    await openFile();

    // Switch to fake timers only after openFile (real-timer) setup has
    // settled, mirroring the neighboring debounce tests in this file (e.g.
    // step-31 above) — this makes the assertion depend on the 500ms debounce
    // firing deterministically rather than racing a real setTimeout against
    // waitFor's 1000ms default under load (task 5155).
    vi.useFakeTimers();
    try {
      const store = capturedDualViewportProps.viewportStore;
      store.setSizeWeight('design-main', 5);
      store.setForceExpanded('design-main', true);
      store.setSplitRatio(0.7);

      // Advance strictly past the 500ms debounce window to flush the write.
      await vi.advanceTimersByTimeAsync(501);

      expect(saveSpy).toHaveBeenCalledWith(
        '/test/bracket.ri',
        expect.objectContaining({
          viewportLayout: expect.objectContaining({
            'design-main': { sizeWeight: 5, forceExpanded: true },
          }),
          splitRatio: expect.closeTo(0.7, 5),
        }),
      );
    } finally {
      // Restore alongside the timer cleanup so spy teardown still happens if
      // the assertion above throws (task 5155 amendment).
      vi.useRealTimers();
      saveSpy.mockRestore();
    }
    void realSave;
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

  // No-over-firing pin for the store-owned tree version (PRD §4.5 B8′): one
  // idle refetch is one regeneration, so one demand push and one prompt.
  it('(h) B8′: an idle refetch fires exactly one syncDemand and one rebind prompt per stale path', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([makeNode('Assembly.flange.geometry')]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    // show → ghost COWs into a user view: the regeneration's user:* branch
    // leaves `explicit` untouched, so only the tree version can re-fire the
    // demand sync.
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));
    await flushMacrotasks(SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS + 50);
    const before = vi.mocked(bridge.syncDemand).mock.calls.length;

    await triggerTreeUpdate(
      evalRef.get()!,
      [makeNode('Assembly.bolt_flange.geometry')],
      2,
    );
    await waitFor(() => screen.getByTestId('toast'));
    await flushMacrotasks(SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS + 50);

    expect(screen.getAllByTestId('toast').length).toBe(1);
    expect(vi.mocked(bridge.syncDemand).mock.calls.length).toBe(before + 1);
  });

  // A prompt closed without an answer waits for the next tree update; an
  // explicit change alone (an unrelated eye toggle) must not bring it back.
  it('(i) a prompt closed with X re-surfaces on the next tree update, not on an eye toggle', async () => {
    const evalRef = captureEvalStatus();
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      makeNode('Assembly.flange.geometry'),
      makeNode('Assembly.shaft'),
    ]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalRef.get()).toBeDefined());
    await waitFor(() => screen.getByTestId('eye-icon-Assembly.flange.geometry'));
    fireEvent.click(screen.getByTestId('eye-icon-Assembly.flange.geometry'));

    const renamedTree = () => [makeNode('Assembly.bolt_flange.geometry'), makeNode('Assembly.shaft')];
    await triggerTreeUpdate(evalRef.get()!, renamedTree(), 2);
    await waitFor(() => screen.getByTestId('toast'));
    fireEvent.click(within(screen.getByTestId('toast')).getByRole('button', { name: /^close$/i }));
    await waitFor(() => expect(screen.queryByTestId('toast')).toBeNull());

    fireEvent.click(screen.getByTestId('eye-icon-Assembly.shaft'));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('toast')).toBeNull();

    await triggerTreeUpdate(evalRef.get()!, renamedTree(), 3);
    await waitFor(() => expect(screen.getAllByTestId('toast').length).toBe(1));
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
